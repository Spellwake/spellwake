//! Native game-scoped input boundary over the restricted executor.
//!
//! Candidates are owned values, not durable commits. This module supplies no auth,
//! clocks, wire/checkpoint codec, seat projection, replay or general Magic engine.
#[cfg(test)]
mod cast_reference_tests;
#[cfg(test)]
pub(crate) mod private_qualification;
#[cfg(test)]
mod related_player_fault_qualification;
#[cfg(test)]
mod token_fault_tests;
use crate::checks::{CheckBoundary, CheckDecision, CheckJournal};
#[cfg(test)]
use crate::combat::CombatRules;
use crate::combat::{CombatAssignmentRecord, CombatDecision, CombatDeclarationRecord};
use crate::effects::{ManaActivationResult, RuleResult};
#[cfg(test)]
use crate::mana::ManaAbilityId;
use crate::occurrence::{
    DrawOrigin, OpeningMovePhase, OpeningShufflePhase, RulesJournal, RulesTransition, TriggerOrder,
    TriggerTargetChoice,
};
#[cfg(test)]
use crate::program::{BodyTarget, ExecutableBody, SpellFilter};
use crate::setup::GameSeed;
#[cfg(test)]
mod action_tests;
mod attack_mana;
#[cfg(test)]
mod characteristics_remaining_tests;
#[cfg(test)]
mod characteristics_tests;
#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod commander_identity_tests;
pub use attack_mana::{AttackManaChoice, AttackManaId};
mod conclusion;
mod counter_replacement;
mod facts;
pub use conclusion::{GameConclusion, GameConclusionCause};
#[cfg(test)]
mod concession_tests;
#[cfg(test)]
mod lifecycle_tests;
mod opening_flow;
use facts::TransitionFacts;
#[cfg(test)]
mod legend_tests;
#[cfg(test)]
mod permanent_tests;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod shared_damage_tests;
#[cfg(test)]
mod world_history_tests;
use crate::actions::{LandPlayRecord, PermanentEntryRecord, TimingError};
use crate::activated::{AbilityActivationRecord, ActivatedAbilityId};
use crate::{
    damage::{DamageJournal, DamageOrigin},
    draw::{DrawJournal, GameOutcome},
    effects::{
        CopyResolution, DamageDecision, EffectDecision, EffectError, EffectWindow,
        ManaActivationProgress, ManaColorChoice, RuleCompletion, RuleProgress,
    },
    mana::{ManaPayment, ManaPaymentRecord, ManaProductionRecord},
    object_identity::{ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{
        PriorityCompleted, Step, TurnError, TurnPosition, TurnScheduler, TurnStage, TurnStart,
        TurnWork, WorkCompleted,
    },
};
#[cfg(test)]
use crate::{effects::SeedCard, mana::FloatingMana};
use crate::{
    opening::{
        HAND_SIZE, MulliganChoice, OpeningDecision, OpeningError, OpeningStage, OpeningState,
    },
    shuffle::{EntropyError, EntropyTape, ShuffleRecord},
};
use std::collections::{BTreeMap, BTreeSet};

/// Host-supplied identity; numeric object/seat identities remain local to this game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameId(pub u128);

/// Owned immutable source identities. Constructors validate shape, not source truth
/// or all-card qualification; the enclosing publisher must establish those claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameContext {
    release: String,
    rules: String,
    catalogue: String,
    programs: String,
    profile: String,
    creature_vocabulary: spellwake_contracts::creature_types::CreatureVocabularyId,
    name_rules: spellwake_contracts::names::NameRulesId,
}
impl GameContext {
    pub fn new(
        release: String,
        rules: String,
        catalogue: String,
        programs: String,
        profile: String,
        creature_vocabulary: spellwake_contracts::creature_types::CreatureVocabularyId,
        name_rules: spellwake_contracts::names::NameRulesId,
    ) -> Result<Self, GameError> {
        if [&release, &rules, &catalogue, &programs, &profile]
            .iter()
            .any(|value| value.trim().is_empty() || value.trim() != value.as_str())
        {
            return Err(GameError::InvalidContext);
        }
        Ok(Self {
            release,
            rules,
            catalogue,
            programs,
            profile,
            creature_vocabulary,
            name_rules,
        })
    }
    pub fn release(&self) -> &str {
        &self.release
    }
    pub fn rules(&self) -> &str {
        &self.rules
    }
    pub fn catalogue(&self) -> &str {
        &self.catalogue
    }
    pub fn programs(&self) -> &str {
        &self.programs
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
    pub fn creature_vocabulary_id(
        &self,
    ) -> spellwake_contracts::creature_types::CreatureVocabularyId {
        self.creature_vocabulary
    }
    pub fn name_rules_id(&self) -> spellwake_contracts::names::NameRulesId {
        self.name_rules
    }
}

/// Exact expected input basis. External accepted-input revision is distinct from
/// the private executor's counters, which can advance several times per input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBasis {
    pub game: GameId,
    pub context: GameContext,
    pub epoch: u64,
    pub revision: u64,
}

/// Decision identity local to its complete game/context/epoch/revision basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionId(pub u64);

/// One canonical native frame/reply vocabulary shared with the executor.
pub use crate::effects::DecisionReply;
/// Game-level choice vocabulary; mana abilities have no stack-resolution frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleFrame {
    Mutation(crate::effects::MutationFrame),
    CounterReplacement(crate::effects::CounterReplacementChoice),
    CastRecovery(crate::effects::CastRecoveryFrame),
    LinkedAuraChoice(crate::effects::LinkedAuraChoiceFrame),
    LinkedReturnOrder(crate::effects::LinkedReturnOrderFrame),
    EntryChoice(crate::effects::EntryChoiceFrame),
    TokenEntryChoice(crate::token::TokenEntryChoiceFrame),
    TokenAuraChoice(crate::token::TokenAuraChoiceFrame),
    /// Privileged native precommit replacement request.
    ZoneReplacement(crate::effects::ZoneReplacementFrame),
    /// Privileged genuine library partition; not a production seat-safe view.
    LibraryTop(crate::effects::LibraryTopFrame),
    /// The executor's exact affected-player selection, under this root's authority.
    ResolutionSelect(crate::effects::ResolutionSelectionFrame),
    /// The executor's complete physical owner order, after all selections.
    ResolutionGraveyardOrder(crate::effects::ResolutionGraveyardOrderFrame),
    OptionalEffect(crate::effects::OptionalEffectFrame),
    ReplacementChooser(crate::effects::ReplacementChooserFrame),
    ResolutionPayment(crate::effects::ResolutionPaymentFrame),
    MyriadDefender(crate::effects::MyriadDefenderFrame),
    ReselectAttackDefender(crate::effects::AttackDefenderFrame),
    TokenDefender(crate::token::TokenDefenderFrame),
    TransformOrder(crate::day_night::TransformOrder),
    UntapKeepTapped(crate::effects::UntapKeepTappedFrame),
    TimestampOrder(crate::token::TokenTimestampOrder),
    Checks(CheckDecision),
    Triggers(TriggerOrder),
    TriggerTarget(TriggerTargetChoice),
    Opening(OpeningDecision),
    CopyTargets(CopyResolution),
    Damage(DamageDecision),
    ManaColor(ManaColorChoice),
    ResolutionManaColor(crate::effects::ResolutionManaColorChoice),
    OptionIndex(crate::effects::OptionIndexFrame),
    RevealedPiles(crate::effects::PileChoiceFrame),
    PlaneswalkerType(crate::effects::PlaneswalkerTypeFrame),
    AttackDirection(crate::effects::AttackDirectionFrame),
    DrawReplacement(crate::effects::DrawReplacementFrame),
    OpponentChoice(crate::effects::OpponentChoiceFrame),
    DamageRecipientChoice(crate::effects::DamageRecipientChoiceFrame),
    DamageSourceChoice(crate::effects::DamageSourceChoiceFrame),
    LoyaltyRemoval(crate::effects::LoyaltyRemovalChoice),
    Combat(CombatDecision),
    Proliferate(crate::effects::ProliferateChoice),
    CounterRemoval(crate::effects::CounterRemovalChoice),
    CreatureType(crate::effects::CreatureTypeChoice),
}
impl RuleFrame {
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::CounterReplacement(choice) => choice.chooser(),
            Self::CastRecovery(f) => f.chooser(),
            Self::LinkedAuraChoice(f) => f.chooser(),
            Self::LinkedReturnOrder(f) => f.chooser(),
            Self::EntryChoice(f) => f.chooser(),
            Self::TokenEntryChoice(f) => f.chooser(),
            Self::TokenAuraChoice(f) => f.chooser(),
            Self::ZoneReplacement(f) => f.chooser(),
            Self::LibraryTop(f) => f.chooser(),
            Self::ResolutionSelect(f) => f.chooser(),
            Self::ResolutionGraveyardOrder(f) => f.chooser(),
            Self::OptionalEffect(f) => f.chooser(),
            Self::ReplacementChooser(f) => f.chooser(),
            Self::ResolutionPayment(f) => f.chooser(),
            Self::MyriadDefender(f) => f.chooser(),
            Self::ReselectAttackDefender(f) => f.chooser(),
            Self::TokenDefender(f) => f.chooser(),
            Self::TransformOrder(f) => f.chooser(),
            Self::UntapKeepTapped(f) => f.chooser(),
            Self::TimestampOrder(f) => f.chooser(),
            Self::Checks(f) => f.chooser(),
            Self::Triggers(f) => f.chooser(),
            Self::TriggerTarget(f) => f.chooser(),
            Self::Opening(f) => f.chooser(),
            Self::CopyTargets(f) => f.chooser(),
            Self::Damage(f) => f.chooser(),
            Self::ManaColor(f) => f.chooser(),
            Self::ResolutionManaColor(f) => f.chooser(),
            Self::OptionIndex(f) => f.chooser(),
            Self::Mutation(f) => f.chooser(),
            Self::RevealedPiles(f) => f.chooser(),
            Self::PlaneswalkerType(f) => f.chooser(),
            Self::AttackDirection(f) => f.chooser(),
            Self::DrawReplacement(f) => f.chooser,
            Self::OpponentChoice(f) => f.chooser(),
            Self::DamageRecipientChoice(f) => f.chooser(),
            Self::DamageSourceChoice(f) => f.chooser(),
            Self::LoyaltyRemoval(f) => f.chooser(),
            Self::Combat(f) => f.chooser(),
            Self::Proliferate(f) => f.chooser(),
            Self::CounterRemoval(f) => f.chooser(),
            Self::CreatureType(f) => f.chooser(),
        }
    }
    fn effect_decision(&self) -> Option<EffectDecision> {
        match self {
            Self::CastRecovery(f) => Some(EffectDecision::CastRecovery(f.clone())),
            Self::LinkedAuraChoice(f) => Some(EffectDecision::LinkedAuraChoice(f.clone())),
            Self::LinkedReturnOrder(f) => Some(EffectDecision::LinkedReturnOrder(f.clone())),
            Self::EntryChoice(f) => Some(EffectDecision::EntryChoice(f.clone())),
            Self::TokenEntryChoice(f) => Some(EffectDecision::TokenEntryChoice(f.clone())),
            Self::TokenAuraChoice(f) => Some(EffectDecision::TokenAuraChoice(f.clone())),
            Self::ZoneReplacement(f) => Some(EffectDecision::ZoneReplacement(f.clone())),
            Self::LibraryTop(f) => Some(EffectDecision::LibraryTop(f.clone())),
            Self::ResolutionSelect(f) => Some(EffectDecision::ResolutionSelect(f.clone())),
            Self::ResolutionGraveyardOrder(f) => {
                Some(EffectDecision::ResolutionGraveyardOrder(f.clone()))
            }
            Self::OptionalEffect(f) => Some(EffectDecision::OptionalEffect(f.clone())),
            Self::ReplacementChooser(f) => Some(EffectDecision::ReplacementChooser(f.clone())),
            Self::ResolutionPayment(f) => Some(EffectDecision::ResolutionPayment(f.clone())),
            Self::MyriadDefender(f) => Some(EffectDecision::MyriadDefender(f.clone())),
            Self::ReselectAttackDefender(f) => {
                Some(EffectDecision::ReselectAttackDefender(f.clone()))
            }
            Self::TokenDefender(f) => Some(EffectDecision::TokenDefender(f.clone())),
            Self::TimestampOrder(f) => Some(EffectDecision::TimestampOrder(f.clone())),
            Self::Checks(f) => Some(EffectDecision::Checks(f.clone())),
            Self::Triggers(f) => Some(EffectDecision::Triggers(f.clone())),
            Self::TriggerTarget(f) => Some(EffectDecision::TriggerTarget(f.clone())),
            Self::CopyTargets(f) => Some(EffectDecision::CopyTargets(f.clone())),
            Self::Damage(f) => Some(EffectDecision::Damage(f.clone())),
            Self::Combat(f) => Some(EffectDecision::Combat(f.clone())),
            Self::Proliferate(f) => Some(EffectDecision::Proliferate(f.clone())),
            Self::CounterRemoval(f) => Some(EffectDecision::CounterRemoval(f.clone())),
            Self::CreatureType(f) => Some(EffectDecision::CreatureType(f.clone())),
            Self::ResolutionManaColor(f) => Some(EffectDecision::ResolutionManaColor(f.clone())),
            Self::OptionIndex(f) => Some(EffectDecision::OptionIndex(f.clone())),
            Self::Mutation(f) => Some(EffectDecision::Mutation(f.clone())),
            Self::RevealedPiles(f) => Some(EffectDecision::RevealedPiles(f.clone())),
            Self::PlaneswalkerType(f) => Some(EffectDecision::PlaneswalkerType(f.clone())),
            Self::AttackDirection(f) => Some(EffectDecision::AttackDirection(f.clone())),
            Self::DrawReplacement(f) => Some(EffectDecision::DrawReplacement(f.clone())),
            Self::OpponentChoice(f) => Some(EffectDecision::OpponentChoice(f.clone())),
            Self::DamageRecipientChoice(f) => {
                Some(EffectDecision::DamageRecipientChoice(f.clone()))
            }
            Self::DamageSourceChoice(f) => Some(EffectDecision::DamageSourceChoice(f.clone())),
            Self::LoyaltyRemoval(f) => Some(EffectDecision::LoyaltyRemoval(f.clone())),
            Self::TransformOrder(f) => Some(EffectDecision::TransformOrder(f.clone())),
            Self::UntapKeepTapped(f) => Some(EffectDecision::UntapKeepTapped(f.clone())),
            Self::Opening(_) | Self::ManaColor(_) | Self::CounterReplacement(_) => None,
        }
    }
}
impl From<EffectDecision> for RuleFrame {
    fn from(frame: EffectDecision) -> Self {
        match frame {
            EffectDecision::ManaColor(f) => Self::ManaColor(f),
            EffectDecision::ResolutionManaColor(f) => Self::ResolutionManaColor(f),
            EffectDecision::OptionIndex(f) => Self::OptionIndex(f),
            EffectDecision::Mutation(f) => Self::Mutation(f),
            EffectDecision::RevealedPiles(f) => Self::RevealedPiles(f),
            EffectDecision::PlaneswalkerType(f) => Self::PlaneswalkerType(f),
            EffectDecision::AttackDirection(f) => Self::AttackDirection(f),
            EffectDecision::DrawReplacement(f) => Self::DrawReplacement(f),
            EffectDecision::OpponentChoice(f) => Self::OpponentChoice(f),
            EffectDecision::DamageRecipientChoice(f) => Self::DamageRecipientChoice(f),
            EffectDecision::DamageSourceChoice(f) => Self::DamageSourceChoice(f),
            EffectDecision::LoyaltyRemoval(f) => Self::LoyaltyRemoval(f),
            EffectDecision::CastRecovery(f) => Self::CastRecovery(f),
            EffectDecision::LinkedAuraChoice(f) => Self::LinkedAuraChoice(f),
            EffectDecision::LinkedReturnOrder(f) => Self::LinkedReturnOrder(f),
            EffectDecision::EntryChoice(f) => Self::EntryChoice(f),
            EffectDecision::TokenEntryChoice(f) => Self::TokenEntryChoice(f),
            EffectDecision::TokenAuraChoice(f) => Self::TokenAuraChoice(f),
            EffectDecision::ZoneReplacement(f) => Self::ZoneReplacement(f),
            EffectDecision::LibraryTop(f) => Self::LibraryTop(f),
            EffectDecision::ResolutionSelect(f) => Self::ResolutionSelect(f),
            EffectDecision::ResolutionGraveyardOrder(f) => Self::ResolutionGraveyardOrder(f),
            EffectDecision::OptionalEffect(f) => Self::OptionalEffect(f),
            EffectDecision::ReplacementChooser(f) => Self::ReplacementChooser(f),
            EffectDecision::ResolutionPayment(f) => Self::ResolutionPayment(f),
            EffectDecision::MyriadDefender(f) => Self::MyriadDefender(f),
            EffectDecision::ReselectAttackDefender(f) => Self::ReselectAttackDefender(f),
            EffectDecision::TokenDefender(f) => Self::TokenDefender(f),
            EffectDecision::TimestampOrder(f) => Self::TimestampOrder(f),
            EffectDecision::TransformOrder(f) => Self::TransformOrder(f),
            EffectDecision::UntapKeepTapped(f) => Self::UntapKeepTapped(f),
            EffectDecision::Checks(f) => Self::Checks(f),
            EffectDecision::Triggers(f) => Self::Triggers(f),
            EffectDecision::TriggerTarget(f) => Self::TriggerTarget(f),
            EffectDecision::CopyTargets(f) => Self::CopyTargets(f),
            EffectDecision::Damage(f) => Self::Damage(f),
            EffectDecision::Combat(f) => Self::Combat(f),
            EffectDecision::Proliferate(f) => Self::Proliferate(f),
            EffectDecision::CounterRemoval(f) => Self::CounterRemoval(f),
            EffectDecision::CreatureType(f) => Self::CreatureType(f),
        }
    }
}

/// Privileged native decision record, not a safe public DecisionView.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionState {
    id: DecisionId,
    basis: GameBasis,
    chooser: SeatId,
    frame: RuleFrame,
}
impl DecisionState {
    pub fn id(&self) -> DecisionId {
        self.id
    }
    pub fn basis(&self) -> &GameBasis {
        &self.basis
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn frame(&self) -> &RuleFrame {
        &self.frame
    }
}

/// Only commands supported by the current closed-world executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameCommand {
    /// Cast a face-down creature using morph/megamorph or disguise for {3}.
    CastFaceDown {
        spell: ObjectId,
        kind: crate::face_down::FaceDownKind,
        payment: Option<ManaPayment>,
    },
    /// Turn this controlled face-down permanent up as a special action.
    TurnFaceUp {
        permanent: ObjectId,
        ability: usize,
        payment: Option<ManaPayment>,
    },
    /// Immediately concede this originally two-player game as the acting seat.
    Concede,
    /// Cast using explicitly chosen alternative payments for individual mana symbols.
    CastWithResources {
        spell: ObjectId,
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
    },
    CastUntargeted {
        spell: ObjectId,
        payment: Option<ManaPayment>,
    },
    /// Exile a card from hand by paying its plot cost at sorcery timing.
    Plot {
        source: ObjectId,
        payment: Option<ManaPayment>,
    },
    Foretell {
        source: ObjectId,
        payment: Option<ManaPayment>,
    },
    Suspend {
        source: ObjectId,
        payment: Option<ManaPayment>,
    },
    PlayLand {
        land: ObjectId,
    },
    /// Play a land and provide its required as-entry color choice.
    PlayLandWithEntryColor {
        land: ObjectId,
        color: crate::mana::ManaType,
    },
    /// Play a land, either revealing the selected hand card or explicitly declining.
    PlayLandWithEntryReveal {
        land: ObjectId,
        reveal: Option<ObjectId>,
    },
    /// Activate a cycling ability of this exact card in the acting player's hand.
    Cycle {
        source: ObjectId,
        ability: ActivatedAbilityId,
        payment: Option<ManaPayment>,
    },
    /// Reveal a card from hand and return the selected unblocked attacker.
    Ninjutsu {
        source: ObjectId,
        ability: ActivatedAbilityId,
        attacker: ObjectId,
        payment: Option<ManaPayment>,
    },
    /// Activate a transmute ability from hand at sorcery timing.
    Transmute {
        source: ObjectId,
        ability: ActivatedAbilityId,
        payment: Option<ManaPayment>,
    },
    /// Activate a nontargeted ability of a permanent on the battlefield.
    ActivateWithCreatures {
        source: ObjectId,
        ability: ActivatedAbilityId,
        creatures: Vec<ObjectId>,
    },
    /// Activate using selected sacrifice/discard cost objects.
    ActivateWithCostObjects {
        source: ObjectId,
        ability: ActivatedAbilityId,
        granting_effect: Option<crate::characteristics::EffectInstanceKey>,
        target: Option<TargetRef>,
        objects: Vec<ObjectId>,
        payment: Option<ManaPayment>,
    },
    /// Activate a nonmana ability using explicitly selected restricted mana.
    ActivateWithRestrictedMana {
        source: ObjectId,
        ability: ActivatedAbilityId,
        granting_effect: Option<crate::characteristics::EffectInstanceKey>,
        origin: crate::activated::ActivationOrigin,
        chosen_x: Option<spellwake_contracts::RulesInteger>,
        payment: Option<ManaPayment>,
        restricted_mana: Vec<crate::mana::RestrictedManaSpend>,
    },
    ActivateUntargetedAbility {
        source: ObjectId,
        ability: ActivatedAbilityId,
        payment: Option<ManaPayment>,
    },
    /// Activate a targeted loyalty ability with a chosen nonnegative X.
    /// Activate the ability currently granted to this exact permanent by the identified continuous effect.
    /// Activate a command-zone emblem by paying mana and sacrificing a controlled creature.
    ActivateEmblem {
        emblem: crate::emblem::EmblemId,
        creature: ObjectId,
        payment: Option<ManaPayment>,
    },
    ActivatePermanentTargets {
        source: ObjectId,
        ability: ActivatedAbilityId,
        targets: Vec<TargetRef>,
        payment: Option<ManaPayment>,
    },
    ActivateGranted {
        source: ObjectId,
        effect: crate::characteristics::EffectInstanceKey,
        target: Option<TargetRef>,
        payment: Option<ManaPayment>,
    },
    ActivateLoyaltyX {
        source: ObjectId,
        ability: ActivatedAbilityId,
        target: TargetRef,
        chosen_x: spellwake_contracts::RulesInteger,
        payment: Option<ManaPayment>,
    },
    ActivateAbility {
        source: ObjectId,
        ability: ActivatedAbilityId,
        target: TargetRef,
        payment: Option<ManaPayment>,
    },
    ActivateModalAbility {
        source: ObjectId,
        ability: ActivatedAbilityId,
        modes: Vec<crate::mana::CastMode>,
        payment: Option<ManaPayment>,
    },
    ActivateMana {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    },
    /// Activate a mana ability with selected objects sacrificed as its cost.
    ActivateManaWithCostObjects {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        objects: Vec<ObjectId>,
    },
    /// Activate a mana ability whose recipient is chosen by its controller.
    ActivateManaForPlayer {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        recipient: SeatId,
    },
    ActivateManaWithRestrictedPayment {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        payment: ManaPayment,
        restricted_mana: Vec<crate::mana::RestrictedManaSpend>,
        bundle: Option<BTreeMap<crate::mana::ManaType, spellwake_contracts::RulesInteger>>,
    },
    /// Pay a hybrid filter cost and choose its exact two-mana output bundle.
    ActivateHybridMana {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        payment: ManaPayment,
        bundle: BTreeMap<crate::mana::ManaType, spellwake_contracts::RulesInteger>,
    },
    /// Activate a tap mana ability with a chosen multi-color bundle.
    ActivateManaWithBundle {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        bundle: BTreeMap<crate::mana::ManaType, spellwake_contracts::RulesInteger>,
    },
    /// Activate a mana ability and explicitly allocate mana to its cost.
    ActivateManaWithPayment {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        payment: ManaPayment,
    },
    Cast {
        spell: ObjectId,
        target: TargetRef,
    },
    CastPaid {
        spell: ObjectId,
        target: TargetRef,
        payment: ManaPayment,
    },
    Pass,
    Reply {
        decision: DecisionId,
        choice: DecisionReply,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInput {
    pub basis: GameBasis,
    pub actor: SeatId,
    pub command: GameCommand,
}
/// Prepared ordinary individual-turn opening after commander/companion setup.
/// No seeded mana, hidden-view contract or full deck legality is implied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningSetup {
    pub seed: GameSeed,
    pub entropy: EntropyTape,
}
enum InitialBoundary {
    Interaction,
    Turn(TurnScheduler),
    Opening(EntropyTape),
}

/// Initialization does not pretend that a previous accepted state existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateBase {
    Initialize { game: GameId, context: GameContext },
    Advance(GameBasis),
}
/// The caller must still serialize and atomically accept this native candidate.
/// It is not a complete event/replay journal or an authorization receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCandidate {
    base: CandidateBase,
    next: GameState,
    facts: TransitionFacts,
}
/// Current-candidate step boundaries, not an archival or seat-safe replay journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnTransition {
    pub from: Option<TurnPosition>,
    pub to: TurnPosition,
    pub active_before: SeatId,
    pub active_after: SeatId,
}
impl NativeCandidate {
    /// Current accepted check choices and physical discard/SBA moves only.
    pub fn check_journal(&self) -> &CheckJournal {
        &self.facts.check_journal
    }
    /// Declaration facts produced only by this successful candidate.
    pub fn combat_declarations(&self) -> &[CombatDeclarationRecord] {
        &self.facts.combat_declarations
    }
    /// Assignment facts produced only by this successful candidate.
    pub fn combat_assignments(&self) -> &[CombatAssignmentRecord] {
        &self.facts.combat_assignments
    }
    pub fn permanent_entry(&self) -> Option<&PermanentEntryRecord> {
        self.facts.permanent_entry.as_ref()
    }
    pub fn land_play(&self) -> Option<&LandPlayRecord> {
        self.facts.land_play.as_ref()
    }
    pub fn shuffle_journal(&self) -> &[ShuffleRecord] {
        &self.facts.shuffle_journal
    }
    pub fn rules_journal(&self) -> &RulesJournal {
        &self.facts.rules_journal
    }
    pub fn draw_journal(&self) -> &DrawJournal {
        &self.facts.draw_journal
    }
    pub fn turn_transitions(&self) -> &[TurnTransition] {
        &self.facts.turn_transitions
    }
    pub fn ability_activation(&self) -> Option<&AbilityActivationRecord> {
        self.facts.ability_activation.as_ref()
    }
    pub fn mana_production(&self) -> Option<&ManaProductionRecord> {
        self.facts.mana_production.as_ref()
    }
    pub fn mana_payment(&self) -> Option<&ManaPaymentRecord> {
        self.facts.mana_payment.as_ref()
    }
    pub fn base(&self) -> &CandidateBase {
        &self.base
    }
    pub fn damage_journal(&self) -> &DamageJournal {
        &self.facts.damage_journal
    }
    pub fn state(&self) -> &GameState {
        &self.next
    }
    /// Adopt for native execution only; this method performs no durable acceptance.
    pub fn into_state(self) -> GameState {
        self.next
    }
}

/// One root owns one executor and its accepted boundary/continuation metadata.
/// Objects, stack, payloads and resources are never duplicated in another store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    basis: GameBasis,
    execution: Box<EffectWindow>,
    attack_mana: Option<std::sync::Arc<attack_mana::AttackManaHistory>>,
    pending: Option<DecisionState>,
    counter_replacement: Option<counter_replacement::SuspendedCounterPlacement>,
    next_decision: u64,
    turn: Option<TurnScheduler>,
    opening: Option<OpeningState>,
    random: Option<EntropyTape>,
    search_reveal: Option<(u64, Vec<crate::object_identity::GameObject>)>,
    face_down_reveals: Vec<crate::object_identity::GameObject>,
    /// Public library disclosures retained while their resolving entry is pending.
    library_reveals: Vec<(
        crate::stack_window::StackEntryId,
        crate::object_identity::GameObject,
    )>,
    cascade_reveals: Vec<crate::object_identity::GameObject>,
    conclusion: Option<GameConclusion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameError {
    Opening(OpeningError),
    Entropy(EntropyError),
    GameEnded,
    /// The suspended game process cannot yet be safely continued after departure.
    UnsupportedConcessionScope,
    PlayerEliminationRequired,
    TurnWorkRequired(TurnWork),
    Turn(TurnError),
    InvalidContext,
    WrongGame,
    WrongContext,
    WrongEpoch,
    StaleRevision,
    UnknownActor,
    DecisionRequired,
    NoDecision,
    WrongDecision,
    WrongChooser,
    RevisionExhausted,
    DecisionIdentityExhausted,
    UnexpectedKernelPhase,
    Effect(EffectError),
}
impl From<OpeningError> for GameError {
    fn from(value: OpeningError) -> Self {
        Self::Opening(value)
    }
}
impl From<EntropyError> for GameError {
    fn from(value: EntropyError) -> Self {
        Self::Entropy(value)
    }
}
impl From<TurnError> for GameError {
    fn from(value: TurnError) -> Self {
        Self::Turn(value)
    }
}
impl From<EffectError> for GameError {
    fn from(value: EffectError) -> Self {
        Self::Effect(value)
    }
}

impl GameState {
    /// Root counter fault injection, absent from production builds.
    #[cfg(test)]
    pub(crate) fn set_test_input_revision(&mut self, revision: u64) {
        self.basis.revision = revision;
        if let Some(pending) = &mut self.pending {
            pending.basis = self.basis.clone();
        }
    }

    /// Decision allocator fault injection, absent from production builds.
    #[cfg(test)]
    pub(crate) fn set_test_next_decision(&mut self, sequence: u64) {
        self.next_decision = sequence;
    }

    /// Private operational counter fork of an actually reached check cut.
    #[cfg(test)]
    pub(crate) fn set_test_check_effect_fault(
        &mut self,
        fault: crate::effects::private_qualification::QualificationFault,
    ) {
        self.execution.qualification_set_fault(fault);
    }

    /// Privileged current allocator counter; actual decisions come from bound receipts.
    pub fn next_decision_sequence(&self) -> u64 {
        self.next_decision
    }

    pub fn initialize(
        game: GameId,
        context: GameContext,
        seed: GameSeed,
    ) -> Result<NativeCandidate, GameError> {
        Self::initialize_boundary(game, context, seed, InitialBoundary::Interaction)
    }
    /// Prepared native turn fixture. Executes supported turn work; missing combat
    /// history suspends. Ordinary cleanup uses exact native decisions. This is not
    /// opening setup or restored state.
    pub fn initialize_scheduled(
        game: GameId,
        context: GameContext,
        seed: GameSeed,
        start: TurnStart,
    ) -> Result<NativeCandidate, GameError> {
        let scheduler = TurnScheduler::new(start, seed.seats.len() == 2)?;
        Self::initialize_boundary(game, context, seed, InitialBoundary::Turn(scheduler))
    }
    /// Start a constructed Commander game with the declared command cards face up.
    /// Card and partner eligibility is determined by the catalogue's pregame deck admission.
    ///
    /// # Errors
    /// Requires 100 cards, one or two command cards and 40 starting life per participant,
    /// in addition to the ordinary opening requirements.
    pub fn initialize_commander_opening(
        game: GameId,
        context: GameContext,
        mut setup: OpeningSetup,
    ) -> Result<NativeCandidate, GameError> {
        let mut designated = BTreeSet::new();
        for &seat in &setup.seed.seats {
            let mut personal = 0;
            let mut commanders = 0;
            for card in setup
                .seed
                .cards
                .iter_mut()
                .filter(|card| card.owner == seat)
            {
                personal += 1;
                if card.destination == crate::object_identity::ObjectDestination::Command {
                    if card
                        .program
                        .commander_color_choice_mut()
                        .is_some_and(|choice| choice.is_none())
                    {
                        return Err(OpeningError::InvalidSetup.into());
                    }
                    commanders += 1;
                    designated.insert(card.card);
                }
            }
            if personal != 100
                || !(1..=2).contains(&commanders)
                || !setup
                    .seed
                    .life
                    .iter()
                    .any(|life| life.player == seat && life.total == 40.into())
            {
                return Err(OpeningError::InvalidSetup.into());
            }
        }
        let mut opening = Self::initialize_opening(game, context, setup)?;
        opening.next.execution.designate_commanders(designated);
        Ok(opening)
    }
    /// Start a Commander opening with the trusted, deck-admitted combined identity for each player.
    /// Partner and pregame color choices must already be included in each set.
    ///
    /// # Errors
    /// Rejects missing, extra or duplicate-seat identity coverage and all invalid Commander setups.
    pub fn initialize_commander_opening_with_identities(
        game: GameId,
        context: GameContext,
        setup: OpeningSetup,
        identities: BTreeMap<SeatId, BTreeSet<crate::characteristics::Color>>,
    ) -> Result<NativeCandidate, GameError> {
        let seats: BTreeSet<_> = setup.seed.seats.iter().copied().collect();
        if identities.keys().copied().collect::<BTreeSet<_>>() != seats
            || seats.len() != setup.seed.seats.len()
        {
            return Err(OpeningError::InvalidSetup.into());
        }
        let mut opening = Self::initialize_commander_opening(game, context, setup)?;
        for card in opening.next.execution.window().cards().iter() {
            if let Some(chosen) = card
                .card_id()
                .and_then(|id| opening.next.execution.commander_chosen_color(id))
                && identities
                    .get(&card.owner())
                    .is_none_or(|colors| !colors.contains(&chosen))
            {
                return Err(OpeningError::InvalidSetup.into());
            }
        }
        opening
            .next
            .execution
            .set_commander_color_identities(identities);
        Ok(opening)
    }
    pub fn initialize_opening(
        game: GameId,
        context: GameContext,
        setup: OpeningSetup,
    ) -> Result<NativeCandidate, GameError> {
        let unique: BTreeSet<_> = setup.seed.seats.iter().copied().collect();
        if !setup.seed.mana.is_empty()
            || !(2..=6).contains(&setup.seed.seats.len())
            || unique.len() != setup.seed.seats.len()
            || !unique.contains(&setup.seed.active)
        {
            return Err(OpeningError::InvalidSetup.into());
        }
        let mut counts = BTreeMap::<SeatId, usize>::new();
        for card in &setup.seed.cards {
            if !unique.contains(&card.owner) {
                return Err(OpeningError::InvalidSetup.into());
            }
            match card.destination {
                crate::object_identity::ObjectDestination::LibraryTop
                | crate::object_identity::ObjectDestination::LibraryBottom => {
                    *counts.entry(card.owner).or_default() += 1
                }
                crate::object_identity::ObjectDestination::Command => {}
                _ => return Err(OpeningError::InvalidSetup.into()),
            }
        }
        if setup
            .seed
            .seats
            .iter()
            .any(|seat| counts.get(seat).copied().unwrap_or(0) < HAND_SIZE)
        {
            return Err(OpeningError::InvalidSetup.into());
        }
        Self::initialize_boundary(
            game,
            context,
            setup.seed,
            InitialBoundary::Opening(setup.entropy),
        )
    }
    fn initialize_boundary(
        game: GameId,
        context: GameContext,
        seed: GameSeed,
        boundary: InitialBoundary,
    ) -> Result<NativeCandidate, GameError> {
        if context.creature_vocabulary != seed.creature_vocabulary.id()
            || context.rules != seed.creature_vocabulary.rules_revision()
            || context.name_rules != seed.name_rules.id()
        {
            return Err(GameError::InvalidContext);
        }
        let base = CandidateBase::Initialize {
            game,
            context: context.clone(),
        };
        let mut facts = TransitionFacts::default();
        let multiplayer = seed.seats.len() > 2;
        let active = seed.active;
        let (turn, opening, random) = match boundary {
            InitialBoundary::Interaction => (None, None, None),
            InitialBoundary::Turn(turn) => (Some(turn), None, None),
            InitialBoundary::Opening(random) => {
                (None, Some(OpeningState::new(multiplayer)), Some(random))
            }
        };
        let mut execution = EffectWindow::new(seed)?;
        if opening.is_some() {
            execution = execution.begin_opening(execution.revision())?;
        } else if let Some(schedule) = &turn {
            execution.enter_speed_turn(schedule.position().number, active);
            let transition = execution.enter_turn_step(
                execution.revision(),
                active,
                schedule.stage() == TurnStage::Priority,
                false,
            )?;
            let (state, (), append) = transition.into_parts();
            facts.rules_journal.append(append);
            execution = state;
            execution.set_calendar_position(schedule.position());
            if schedule.position().step == Step::Untap {
                execution = execution.record_turn_start(execution.revision())?;
            }
            facts.turn_transitions.push(TurnTransition {
                from: None,
                to: schedule.position(),
                active_before: active,
                active_after: active,
            });
        }
        let mut next = Self {
            basis: GameBasis {
                game,
                context,
                epoch: 0,
                revision: 0,
            },
            execution: Box::new(execution),
            search_reveal: None,
            face_down_reveals: Vec::new(),
            library_reveals: Vec::new(),
            cascade_reveals: Vec::new(),
            pending: None,
            counter_replacement: None,
            next_decision: 1,
            turn,
            opening,
            random,
            attack_mana: None,
            conclusion: None,
        };
        if next.opening.is_some() {
            let order = next.opening_order()?;
            for &player in &order {
                next.shuffle_opening(player, OpeningShufflePhase::Initial, &mut facts)?;
            }
            for player in order {
                next.draw_opening(player, &mut facts)?;
            }
        }
        let mut next = next.drive(&mut facts)?;
        next.capture_face_down_reveals(&facts.rules_journal);
        next.finalize_checked_losses();
        Ok(NativeCandidate { base, next, facts })
    }
    pub fn basis(&self) -> &GameBasis {
        &self.basis
    }
    /// The first card publicly revealed by a library search at this revision.
    /// Its original incarnation discloses no handle in its new hidden zone.
    pub fn search_reveal(
        &self,
    ) -> Option<(
        &crate::object_identity::GameObject,
        &crate::program::Program,
    )> {
        self.search_reveals().into_iter().next()
    }
    /// Every card publicly revealed by the most recent search at this revision.
    pub fn search_reveals(
        &self,
    ) -> Vec<(
        &crate::object_identity::GameObject,
        &crate::program::Program,
    )> {
        let Some((revision, cards)) = self.search_reveal.as_ref() else {
            return Vec::new();
        };
        if *revision != self.basis.revision {
            return Vec::new();
        }
        cards
            .iter()
            .filter_map(|card| {
                let id = card.card_id()?;
                self.execution
                    .programs()
                    .find(|(key, _)| *key == id)
                    .map(|(_, program)| (card, program))
            })
            .collect()
    }
    fn capture_face_down_reveals(&mut self, journal: &RulesJournal) {
        self.face_down_reveals.clear();
        self.cascade_reveals.clear();
        let resolving = self.execution.window().resolving().map(|entry| entry.id());
        self.library_reveals
            .retain(|(entry, _)| Some(*entry) == resolving);
        for record in &journal.records {
            if let crate::occurrence::RulesRecord::Event(event) = record {
                if let crate::occurrence::EventCause::Instruction { entry, .. } = &event.cause {
                    for row in &event.occurrences {
                        if let crate::occurrence::RulesOccurrence::LibraryCardRevealed { object } =
                            &row.value
                            && !self.library_reveals.iter().any(|(known_entry, known)| {
                                known_entry == entry && known.id() == object.id()
                            })
                        {
                            self.library_reveals.push((*entry, object.clone()));
                        }
                    }
                }
                self.cascade_reveals
                    .extend(event.occurrences.iter().filter_map(|row| match &row.value {
                        crate::occurrence::RulesOccurrence::ZoneChange(moved)
                            if moved.reason == crate::occurrence::ZoneReason::CascadeExile =>
                        {
                            Some(moved.after.object.clone())
                        }
                        _ => None,
                    }));
                self.face_down_reveals
                    .extend(event.occurrences.iter().filter_map(|row| match &row.value {
                        crate::occurrence::RulesOccurrence::LibraryCardRevealed { object }
                            if matches!(
                                event.cause,
                                crate::occurrence::EventCause::Draw { .. }
                            ) =>
                        {
                            Some(object.clone())
                        }
                        crate::occurrence::RulesOccurrence::FaceDownRevealed { object }
                        | crate::occurrence::RulesOccurrence::HandCardRevealed { object } => {
                            Some(object.clone())
                        }
                        _ => None,
                    }));
            }
        }
        for card in self.execution.miracle_reveals() {
            if !self
                .face_down_reveals
                .iter()
                .any(|known| known.id() == card.id())
            {
                self.face_down_reveals.push(card.clone());
            }
        }
        for card in self.execution.revealed_cast_cost_cards() {
            if !self
                .face_down_reveals
                .iter()
                .any(|known| known.id() == card.id())
            {
                self.face_down_reveals.push(card.clone());
            }
        }
        self.face_down_reveals.extend(
            self.library_reveals
                .iter()
                .map(|(_, object)| object.clone()),
        );
    }

    /// Faces publicly exiled by cascade in the most recently accepted operation, in actual reveal order.
    /// Historical exile incarnations never supply a handle into the player's library.
    pub fn cascade_reveals(
        &self,
    ) -> impl Iterator<
        Item = (
            &crate::object_identity::GameObject,
            &crate::program::Program,
        ),
    > {
        self.cascade_reveals
            .iter()
            .filter_map(|o| self.execution.printed_face(o).map(|p| (o, p)))
    }
    /// Previously hidden cards disclosed at this cut, paired with their printed face.
    /// The revealed incarnation carries no public handle into its destination zone.
    pub fn face_down_reveals(
        &self,
    ) -> impl Iterator<
        Item = (
            &crate::object_identity::GameObject,
            &crate::program::Program,
        ),
    > {
        self.face_down_reveals
            .iter()
            .chain(
                self.conclusion
                    .iter()
                    .flat_map(|result| result.face_down_reveals()),
            )
            .filter_map(|object| {
                self.execution
                    .printed_face(object)
                    .map(|face| (object, face))
            })
    }
    /// Current active choice; terminal roots retain only inactive evidence.
    pub fn pending_decision(&self) -> Option<&DecisionState> {
        if self.conclusion.is_some() {
            None
        } else {
            self.pending.as_ref()
        }
    }
    /// Original inactive decision retained after game end; never rebound as a capability.
    pub fn retained_terminal_decision(&self) -> Option<&DecisionState> {
        self.conclusion.as_ref().and(self.pending.as_ref())
    }
    /// Exact root-owned result/cause, available only after actual game conclusion.
    pub fn conclusion(&self) -> Option<&GameConclusion> {
        self.conclusion.as_ref()
    }
    /// Privileged native observation, not a seat-safe projection or mutable handle.
    pub fn execution(&self) -> &EffectWindow {
        &self.execution
    }
    pub fn turn(&self) -> Option<&TurnScheduler> {
        self.turn.as_ref()
    }
    pub fn opening(&self) -> Option<&OpeningState> {
        self.opening.as_ref()
    }
    pub fn entropy(&self) -> Option<&EntropyTape> {
        self.random.as_ref()
    }
    /// Active scheduler work; inspect the retained turn for historical terminal work.
    pub fn pending_turn_work(&self) -> Option<TurnWork> {
        if self.conclusion.is_some() {
            return None;
        }
        match self.turn.as_ref().map(TurnScheduler::stage) {
            Some(TurnStage::Work(work)) => Some(work),
            _ => None,
        }
    }
    /// Winner or draw from the common root conclusion.
    pub fn outcome(&self) -> Option<GameOutcome> {
        self.conclusion.as_ref().map(GameConclusion::outcome)
    }
    /// Checked losses still awaiting a departure or a root conclusion.
    pub fn pending_eliminations(&self) -> Vec<SeatId> {
        if self.conclusion.is_none() {
            self.execution
                .state_based_losses()
                .iter()
                .copied()
                .collect()
        } else {
            vec![]
        }
    }

    /// Tentatively apply one input, preserving the accepted root on rejection.
    ///
    /// # Errors
    /// Validates basis, actor and terminal status before command-specific work.
    /// Concession additionally rejects root revision exhaustion and unsupported
    /// suspended continuations.
    pub fn advance(&self, input: GameInput) -> Result<NativeCandidate, GameError> {
        self.advance_with_attack_mana(input)
    }

    fn advance_inner(&self, input: GameInput) -> Result<NativeCandidate, GameError> {
        self.validate_basis(&input.basis)?;
        if !self
            .execution
            .window()
            .participants()
            .contains(&input.actor)
        {
            return Err(GameError::UnknownActor);
        }
        if self.outcome().is_some() {
            return Err(GameError::GameEnded);
        }
        if matches!(input.command, GameCommand::Concede) {
            return self.concede(input.actor);
        }
        if !self.execution.state_based_losses().is_empty() {
            return Err(GameError::PlayerEliminationRequired);
        }
        let revision = self
            .basis
            .revision
            .checked_add(1)
            .ok_or(GameError::RevisionExhausted)?;
        if let Some(work) = self.pending_turn_work() {
            let work_reply = matches!(&input.command, GameCommand::Reply { .. })
                && self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| self.frame_matches_turn_work(&pending.frame, work));
            let attack_mana = work == TurnWork::DeclareAttackers
                && self.execution.is_paying_attack_cost()
                && matches!(
                    input.command,
                    GameCommand::ActivateMana { .. }
                        | GameCommand::ActivateHybridMana { .. }
                        | GameCommand::ActivateManaWithBundle { .. }
                        | GameCommand::ActivateManaWithCostObjects { .. }
                        | GameCommand::ActivateManaForPlayer { .. }
                        | GameCommand::ActivateManaWithRestrictedPayment { .. }
                        | GameCommand::ActivateManaWithPayment { .. }
                );
            if !work_reply && !attack_mana {
                return Err(GameError::TurnWorkRequired(work));
            }
        }
        let mut next = self.clone();
        let mut facts = TransitionFacts::default();
        let mut mana_progress = None;
        let progress = match (&self.pending, input.command) {
            (Some(pending), GameCommand::Reply { decision, choice }) => {
                if decision != pending.id {
                    return Err(GameError::WrongDecision);
                }
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                if self.pending_turn_work() == Some(TurnWork::Draw)
                    && let Some(frame) = pending.frame.effect_decision()
                    && self.execution.draw_decision() == Some(frame.clone())
                {
                    next.execution.accept_draw_decision(
                        self.execution.revision(),
                        &frame,
                        input.actor,
                        choice,
                    )?;
                    next.pending = None;
                    None
                } else if let RuleFrame::TransformOrder(frame) = &pending.frame
                    && self.pending_turn_work() == Some(TurnWork::Untap)
                {
                    let DecisionReply::TransformOrder(objects) = choice else {
                        return Err(EffectError::WrongDecisionKind.into());
                    };
                    next.execution.choose_transform_order(frame, objects)?;
                    next.pending = None;
                    None
                } else if let RuleFrame::UntapKeepTapped(frame) = &pending.frame
                    && self.pending_turn_work() == Some(TurnWork::Untap)
                {
                    let DecisionReply::UntapKeepTapped(objects) = choice else {
                        return Err(EffectError::WrongDecisionKind.into());
                    };
                    next.execution.choose_untap_keep_tapped(frame, objects)?;
                    next.pending = None;
                    None
                } else if let RuleFrame::Opening(frame) = &pending.frame {
                    next.reply_opening(frame, choice)?;
                    next.pending = None;
                    None
                } else if let RuleFrame::LibraryTop(frame) = &pending.frame
                    && frame.chooses_land_kind()
                {
                    let DecisionReply::ChooseLandKind(land) = choice else {
                        return Err(EffectError::WrongDecisionKind.into());
                    };
                    let empty = EntropyTape::new(vec![]);
                    let (progress, entropy) = self.execution.resume_reveal_land_kind(
                        self.execution.revision(),
                        frame,
                        input.actor,
                        land,
                        self.random.as_ref().unwrap_or(&empty),
                    )?;
                    next.random = Some(entropy);
                    Some(progress)
                } else if let RuleFrame::LibraryTop(frame) = &pending.frame
                    && frame.is_look_top()
                {
                    let selected = match choice {
                        DecisionReply::LibraryTop(crate::effects::LibraryTopReply::Search {
                            selected,
                        }) => selected.into_iter().collect(),
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchBattlefield {
                                selected_in_timestamp_order,
                            },
                        ) if frame.is_battlefield_batch_search() => selected_in_timestamp_order,
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchWithEntryColors {
                                selected,
                                colors,
                            },
                        ) if colors.is_empty() => selected,
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchWithEntryChoices {
                                selected,
                                choices,
                            },
                        ) if choices.is_empty() => selected,
                        _ => return Err(EffectError::WrongDecisionKind.into()),
                    };
                    let revealed = if frame.looked_card_is_revealed() && !selected.is_empty() {
                        Some(
                            selected
                                .iter()
                                .map(|id| {
                                    self.execution
                                        .window()
                                        .cards()
                                        .get(*id)
                                        .cloned()
                                        .ok_or(EffectError::UnknownObject)
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                    } else {
                        None
                    };
                    let empty = EntropyTape::new(vec![]);
                    let (progress, entropy) = self.execution.resume_look_top(
                        self.execution.revision(),
                        frame,
                        input.actor,
                        selected,
                        self.random.as_ref().unwrap_or(&empty),
                    )?;
                    next.search_reveal = revealed.map(|cards| (revision, cards));
                    next.random = Some(entropy);
                    Some(progress)
                } else if let RuleFrame::LibraryTop(frame) = &pending.frame
                    && frame.kind() == crate::effects::LibraryChoiceKind::Search
                {
                    let empty = EntropyTape::new(vec![]);
                    let tape = self.random.as_ref().unwrap_or(&empty);
                    let (progress, shuffled, selected) = match choice {
                        DecisionReply::LibraryTop(crate::effects::LibraryTopReply::Search {
                            selected,
                        }) => {
                            let (progress, shuffled) = self.execution.resume_search(
                                self.execution.revision(),
                                frame,
                                input.actor,
                                selected,
                                &[],
                                &[],
                                tape,
                            )?;
                            (progress, shuffled, selected.into_iter().collect::<Vec<_>>())
                        }
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchHandGraveyard {
                                hand,
                                graveyard,
                            },
                        ) => {
                            let (progress, shuffled) =
                                self.execution.resume_hand_graveyard_search(
                                    self.execution.revision(),
                                    frame,
                                    input.actor,
                                    hand,
                                    graveyard,
                                    tape,
                                )?;
                            (
                                progress,
                                shuffled,
                                hand.into_iter().chain(graveyard).collect(),
                            )
                        }
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchSplit { battlefield, hand },
                        ) => {
                            let (progress, shuffled) = self.execution.resume_split_search(
                                self.execution.revision(),
                                frame,
                                input.actor,
                                (battlefield, hand),
                                &[],
                                &[],
                                tape,
                            )?;
                            (
                                progress,
                                shuffled,
                                battlefield.into_iter().chain(hand).collect(),
                            )
                        }
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchBattlefield {
                                selected_in_timestamp_order,
                            },
                        ) => {
                            let (progress, shuffled) = self.execution.resume_battlefield_search(
                                self.execution.revision(),
                                frame,
                                input.actor,
                                selected_in_timestamp_order.clone(),
                                &[],
                                &[],
                                tape,
                            )?;
                            (progress, shuffled, selected_in_timestamp_order)
                        }
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchWithEntryColors {
                                selected,
                                colors,
                            },
                        ) => {
                            let (progress, shuffled) =
                                if frame.is_bounded_batch_search() && colors.is_empty() {
                                    self.execution.resume_bounded_batch_search(
                                        self.execution.revision(),
                                        frame,
                                        input.actor,
                                        selected.clone(),
                                        tape,
                                    )?
                                } else if frame.is_split_search() && selected.len() <= 2 {
                                    self.execution.resume_split_search(
                                        self.execution.revision(),
                                        frame,
                                        input.actor,
                                        (selected.first().copied(), selected.get(1).copied()),
                                        &colors,
                                        &[],
                                        tape,
                                    )?
                                } else if frame.is_battlefield_batch_search() {
                                    self.execution.resume_battlefield_search(
                                        self.execution.revision(),
                                        frame,
                                        input.actor,
                                        selected.clone(),
                                        &colors,
                                        &[],
                                        tape,
                                    )?
                                } else if selected.len() <= 1 {
                                    self.execution.resume_search(
                                        self.execution.revision(),
                                        frame,
                                        input.actor,
                                        selected.first().copied(),
                                        &colors,
                                        &[],
                                        tape,
                                    )?
                                } else {
                                    return Err(EffectError::WrongDecisionKind.into());
                                };
                            (progress, shuffled, selected)
                        }
                        DecisionReply::LibraryTop(
                            crate::effects::LibraryTopReply::SearchWithEntryChoices {
                                selected,
                                choices,
                            },
                        ) => {
                            let mut colors = Vec::new();
                            let mut reveals = Vec::new();
                            for (object, choice) in choices {
                                match choice {
                                    crate::land::EntryChoice::Color(color) => {
                                        colors.push((object, color))
                                    }
                                    crate::land::EntryChoice::Reveal(card) => {
                                        reveals.push((object, card))
                                    }
                                    crate::land::EntryChoice::Opponent(_)
                                    | crate::land::EntryChoice::Players(_)
                                    | crate::land::EntryChoice::Chapter(_)
                                    | crate::land::EntryChoice::CreatureType(_)
                                    | crate::land::EntryChoice::CardName(_)
                                    | crate::land::EntryChoice::PayLife(_)
                                    | crate::land::EntryChoice::OptionIndex(_) => {
                                        return Err(EffectError::WrongDecisionKind.into());
                                    }
                                }
                            }
                            let (progress, shuffled) = if frame.is_bounded_batch_search()
                                && colors.is_empty()
                                && reveals.is_empty()
                            {
                                self.execution.resume_bounded_batch_search(
                                    self.execution.revision(),
                                    frame,
                                    input.actor,
                                    selected.clone(),
                                    tape,
                                )?
                            } else if frame.is_split_search() && selected.len() <= 2 {
                                self.execution.resume_split_search(
                                    self.execution.revision(),
                                    frame,
                                    input.actor,
                                    (selected.first().copied(), selected.get(1).copied()),
                                    &colors,
                                    &reveals,
                                    tape,
                                )?
                            } else if frame.is_battlefield_batch_search() {
                                self.execution.resume_battlefield_search(
                                    self.execution.revision(),
                                    frame,
                                    input.actor,
                                    selected.clone(),
                                    &colors,
                                    &reveals,
                                    tape,
                                )?
                            } else if selected.len() <= 1 {
                                self.execution.resume_search(
                                    self.execution.revision(),
                                    frame,
                                    input.actor,
                                    selected.first().copied(),
                                    &colors,
                                    &reveals,
                                    tape,
                                )?
                            } else {
                                return Err(EffectError::WrongDecisionKind.into());
                            };
                            (progress, shuffled, selected)
                        }
                        _ => return Err(EffectError::WrongDecisionKind.into()),
                    };
                    let reveal_selected = self.execution.search_reveals_selected()?;
                    let revealed = if reveal_selected {
                        selected
                            .into_iter()
                            .map(|id| {
                                self.execution
                                    .window()
                                    .cards()
                                    .get(id)
                                    .cloned()
                                    .ok_or(EffectError::UnknownObject)
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        Vec::new()
                    };
                    next.search_reveal = (!revealed.is_empty()).then_some((revision, revealed));
                    if let Some((entropy, shuffle)) = shuffled {
                        next.random = Some(entropy);
                        facts.shuffle_journal.push(shuffle);
                    }
                    Some(progress)
                } else if let RuleFrame::ManaColor(frame) = &pending.frame {
                    let DecisionReply::ManaColor(color) = choice else {
                        return Err(EffectError::WrongDecisionKind.into());
                    };
                    let completed = self.execution.resume_mana_color_with_rules(
                        self.execution.revision(),
                        frame,
                        input.actor,
                        color,
                    )?;
                    match completed {
                        crate::effects::ManaColorResolution::Complete(completed) => {
                            mana_progress = Some(completed.map_result(|production| {
                                ManaActivationResult::Complete { production }
                            }));
                            None
                        }
                        crate::effects::ManaColorResolution::Pending(progress) => {
                            mana_progress = Some(progress);
                            None
                        }
                        crate::effects::ManaColorResolution::Damage {
                            production,
                            progress,
                        } => {
                            facts.mana_production = Some(production);
                            Some(progress)
                        }
                    }
                } else {
                    Some(
                        self.execution.resume_rule(
                            self.execution.revision(),
                            &pending
                                .frame
                                .effect_decision()
                                .ok_or(EffectError::WrongDecisionKind)?,
                            input.actor,
                            choice,
                            self.turn.as_ref(),
                        )?,
                    )
                }
            }
            (Some(pending), GameCommand::ActivateMana { source, ability })
                if self.execution.is_paying_attack_cost()
                    || matches!(
                        pending.frame,
                        RuleFrame::ResolutionPayment(_) | RuleFrame::CastRecovery(_)
                    ) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(self.execution.begin_mana_activation(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                )?);
                None
            }
            (
                Some(pending),
                GameCommand::ActivateManaWithCostObjects {
                    source,
                    ability,
                    objects,
                },
            ) if self.execution.is_paying_attack_cost()
                || matches!(
                    pending.frame,
                    RuleFrame::ResolutionPayment(_) | RuleFrame::CastRecovery(_)
                ) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(self.execution.begin_mana_activation_with_cost_objects(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &objects,
                )?);
                None
            }
            (
                Some(pending),
                GameCommand::ActivateManaForPlayer {
                    source,
                    ability,
                    recipient,
                },
            ) if self.execution.is_paying_attack_cost()
                || matches!(
                    pending.frame,
                    RuleFrame::ResolutionPayment(_) | RuleFrame::CastRecovery(_)
                ) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(self.execution.begin_mana_activation_for_player(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    recipient,
                )?);
                None
            }
            (
                Some(pending),
                GameCommand::ActivateManaWithRestrictedPayment {
                    source,
                    ability,
                    payment,
                    restricted_mana,
                    bundle,
                },
            ) if self.execution.is_paying_attack_cost()
                || matches!(
                    pending.frame,
                    RuleFrame::ResolutionPayment(_) | RuleFrame::CastRecovery(_)
                ) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(
                    self.execution
                        .begin_mana_activation_with_restricted_payment(
                            self.execution.revision(),
                            input.actor,
                            source,
                            ability,
                            &payment,
                            &restricted_mana,
                            bundle.as_ref(),
                        )?,
                );
                None
            }
            (None, GameCommand::Concede) => return Err(GameError::UnexpectedKernelPhase),
            (
                Some(pending),
                GameCommand::ActivateManaWithPayment {
                    source,
                    ability,
                    payment,
                },
            ) if self.execution.is_paying_attack_cost()
                || matches!(
                    pending.frame,
                    RuleFrame::ResolutionPayment(_) | RuleFrame::CastRecovery(_)
                ) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(self.execution.begin_mana_activation_with_payment(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &payment,
                )?);
                None
            }
            (
                Some(pending),
                GameCommand::ActivateHybridMana {
                    source,
                    ability,
                    payment,
                    bundle,
                },
            ) if self.execution.is_paying_attack_cost()
                || matches!(
                    pending.frame,
                    RuleFrame::ResolutionPayment(_) | RuleFrame::CastRecovery(_)
                ) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(self.execution.begin_hybrid_mana_activation(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &payment,
                    &bundle,
                )?);
                None
            }
            (
                Some(pending),
                GameCommand::ActivateManaWithBundle {
                    source,
                    ability,
                    bundle,
                },
            ) if self.execution.is_paying_attack_cost()
                || matches!(
                    pending.frame,
                    RuleFrame::ResolutionPayment(_) | RuleFrame::CastRecovery(_)
                ) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(self.execution.begin_mana_activation_with_bundle(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &bundle,
                )?);
                None
            }
            (Some(_), _) => return Err(GameError::DecisionRequired),
            (None, GameCommand::Reply { .. }) => return Err(GameError::NoDecision),
            (None, GameCommand::Pass) => {
                next.execution = Box::new(
                    self.execution
                        .pass(self.execution.revision(), input.actor)?,
                );
                None
            }
            (
                None,
                command @ (GameCommand::Plot { .. }
                | GameCommand::Foretell { .. }
                | GameCommand::Suspend { .. }),
            ) => {
                let (action, source, payment) = match command {
                    GameCommand::Plot { source, payment } => {
                        (crate::effects::ExileHandAction::Plot, source, payment)
                    }
                    GameCommand::Foretell { source, payment } => {
                        (crate::effects::ExileHandAction::Foretell, source, payment)
                    }
                    GameCommand::Suspend { source, payment } => {
                        (crate::effects::ExileHandAction::Suspend, source, payment)
                    }
                    _ => return Err(GameError::UnexpectedKernelPhase),
                };
                let turn = self
                    .turn
                    .as_ref()
                    .ok_or(EffectError::Timing(TimingError::TurnRequired))?;
                let transition = self.execution.exile_from_hand(
                    action,
                    self.execution.revision(),
                    input.actor,
                    source,
                    payment.as_ref(),
                    turn,
                )?;
                next.accept_execution(transition, &mut facts);
                None
            }
            (
                None,
                GameCommand::Ninjutsu {
                    source,
                    ability,
                    attacker,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        source,
                        ability,
                        chosen_x: None,
                        origin: crate::activated::ActivationOrigin::Ninjutsu(attacker),
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (None, GameCommand::PlayLand { land }) => {
                let turn = self
                    .turn
                    .as_ref()
                    .ok_or(EffectError::Timing(TimingError::TurnRequired))?;
                let schedule =
                    turn.record_land_play(&self.execution.land_play_allowance(input.actor)?)?;
                let progress = self.execution.begin_land_play(
                    self.execution.revision(),
                    input.actor,
                    land,
                    turn,
                    None,
                )?;
                next.turn = Some(schedule);
                Some(progress)
            }
            (None, GameCommand::PlayLandWithEntryColor { land, color }) => {
                let turn = self
                    .turn
                    .as_ref()
                    .ok_or(EffectError::Timing(TimingError::TurnRequired))?;
                let schedule =
                    turn.record_land_play(&self.execution.land_play_allowance(input.actor)?)?;
                let progress = self.execution.begin_land_play(
                    self.execution.revision(),
                    input.actor,
                    land,
                    turn,
                    Some(crate::land::EntryChoice::Color(color)),
                )?;
                next.turn = Some(schedule);
                Some(progress)
            }
            (None, GameCommand::PlayLandWithEntryReveal { land, reveal }) => {
                let turn = self
                    .turn
                    .as_ref()
                    .ok_or(EffectError::Timing(TimingError::TurnRequired))?;
                let schedule =
                    turn.record_land_play(&self.execution.land_play_allowance(input.actor)?)?;
                let progress = self.execution.begin_land_play(
                    self.execution.revision(),
                    input.actor,
                    land,
                    turn,
                    Some(crate::land::EntryChoice::Reveal(reveal)),
                )?;
                next.turn = Some(schedule);
                Some(progress)
            }
            (
                None,
                GameCommand::ActivateEmblem {
                    emblem,
                    creature,
                    payment,
                },
            ) => Some(next.execution.activate_sacrifice_emblem(
                input.actor,
                emblem,
                creature,
                payment.as_ref(),
                next.turn.as_ref(),
            )?),
            (
                None,
                GameCommand::ActivatePermanentTargets {
                    source,
                    ability,
                    targets,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        source,
                        ability,
                        chosen_x: None,
                        granting_effect: None,
                        origin: crate::activated::ActivationOrigin::PermanentTargets(targets),
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateWithRestrictedMana {
                    source,
                    ability,
                    granting_effect,
                    origin,
                    chosen_x,
                    payment,
                    restricted_mana,
                },
            ) => {
                next.activate_for_input_restricted(
                    input.actor,
                    crate::activated::ActivationRequest {
                        source,
                        ability,
                        granting_effect,
                        chosen_x,
                        origin,
                    },
                    payment,
                    &restricted_mana,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateWithCostObjects {
                    source,
                    ability,
                    granting_effect,
                    target,
                    objects,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect,
                        source,
                        ability,
                        chosen_x: None,
                        origin: crate::activated::ActivationOrigin::SelectedCost {
                            target,
                            objects,
                        },
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateGranted {
                    source,
                    effect,
                    target,
                    payment,
                },
            ) => {
                let origin = target.map_or(
                    crate::activated::ActivationOrigin::Untargeted,
                    crate::activated::ActivationOrigin::Targeted,
                );
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        source,
                        ability: ActivatedAbilityId(0),
                        chosen_x: None,
                        granting_effect: Some(effect),
                        origin,
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateLoyaltyX {
                    source,
                    ability,
                    target,
                    chosen_x,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        source,
                        ability,
                        origin: crate::activated::ActivationOrigin::Targeted(target),
                        chosen_x: Some(chosen_x),
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateAbility {
                    source,
                    ability,
                    target,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        chosen_x: None,
                        source,
                        ability,
                        origin: crate::activated::ActivationOrigin::Targeted(target),
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateModalAbility {
                    source,
                    ability,
                    modes,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        chosen_x: None,
                        source,
                        ability,
                        origin: crate::activated::ActivationOrigin::Modal(modes),
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateUntargetedAbility {
                    source,
                    ability,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        chosen_x: None,
                        source,
                        ability,
                        origin: crate::activated::ActivationOrigin::Untargeted,
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::Cycle {
                    source,
                    ability,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        chosen_x: None,
                        source,
                        ability,
                        origin: crate::activated::ActivationOrigin::Cycling,
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::Transmute {
                    source,
                    ability,
                    payment,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        chosen_x: None,
                        source,
                        ability,
                        origin: crate::activated::ActivationOrigin::Transmute,
                    },
                    payment,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateWithCreatures {
                    source,
                    ability,
                    creatures,
                },
            ) => {
                next.activate_for_input(
                    input.actor,
                    crate::activated::ActivationRequest {
                        granting_effect: None,
                        chosen_x: None,
                        source,
                        ability,
                        origin: crate::activated::ActivationOrigin::Contributors(creatures),
                    },
                    None,
                    &mut facts,
                )?;
                None
            }
            (
                None,
                GameCommand::ActivateManaWithBundle {
                    source,
                    ability,
                    bundle,
                },
            ) => {
                mana_progress = Some(self.execution.begin_mana_activation_with_bundle(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &bundle,
                )?);
                None
            }
            (
                None,
                GameCommand::ActivateHybridMana {
                    source,
                    ability,
                    payment,
                    bundle,
                },
            ) => {
                mana_progress = Some(self.execution.begin_hybrid_mana_activation(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &payment,
                    &bundle,
                )?);
                None
            }
            (
                None,
                GameCommand::ActivateManaWithPayment {
                    source,
                    ability,
                    payment,
                },
            ) => {
                mana_progress = Some(self.execution.begin_mana_activation_with_payment(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &payment,
                )?);
                None
            }
            (None, GameCommand::ActivateMana { source, ability }) => {
                mana_progress = Some(self.execution.begin_mana_activation(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                )?);
                None
            }
            (
                None,
                GameCommand::ActivateManaWithCostObjects {
                    source,
                    ability,
                    objects,
                },
            ) => {
                mana_progress = Some(self.execution.begin_mana_activation_with_cost_objects(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    &objects,
                )?);
                None
            }
            (
                None,
                GameCommand::ActivateManaForPlayer {
                    source,
                    ability,
                    recipient,
                },
            ) => {
                mana_progress = Some(self.execution.begin_mana_activation_for_player(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    recipient,
                )?);
                None
            }
            (
                None,
                GameCommand::ActivateManaWithRestrictedPayment {
                    source,
                    ability,
                    payment,
                    restricted_mana,
                    bundle,
                },
            ) => {
                mana_progress = Some(
                    self.execution
                        .begin_mana_activation_with_restricted_payment(
                            self.execution.revision(),
                            input.actor,
                            source,
                            ability,
                            &payment,
                            &restricted_mana,
                            bundle.as_ref(),
                        )?,
                );
                None
            }
            (
                None,
                GameCommand::CastWithResources {
                    spell,
                    target,
                    payment,
                },
            ) => {
                let cast = self.execution.cast_with_resources(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    target,
                    payment,
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                if !next.execution.has_deferred_payment() {
                    facts.mana_payment = Some(payment);
                }
                None
            }
            (
                None,
                GameCommand::CastFaceDown {
                    spell,
                    kind,
                    payment,
                },
            ) => {
                let cast = self.execution.cast_face_down(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    kind,
                    payment,
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                if !next.execution.has_deferred_payment() {
                    facts.mana_payment = Some(payment);
                }
                None
            }
            (
                None,
                GameCommand::TurnFaceUp {
                    permanent,
                    ability,
                    payment,
                },
            ) => {
                let action = self.execution.turn_face_up(
                    self.execution.revision(),
                    input.actor,
                    permanent,
                    ability,
                    payment.as_ref(),
                    self.turn.as_ref(),
                )?;
                next.accept_execution(action, &mut facts);
                None
            }
            (None, GameCommand::Cast { spell, target }) => {
                let cast = self.execution.cast_on_turn(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    Some(target),
                    None,
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                if !next.execution.has_deferred_payment() {
                    facts.mana_payment = Some(payment);
                }
                None
            }
            (
                None,
                GameCommand::CastPaid {
                    spell,
                    target,
                    payment,
                },
            ) => {
                let cast = self.execution.cast_on_turn(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    Some(target),
                    Some(payment),
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                if !next.execution.has_deferred_payment() {
                    facts.mana_payment = Some(payment);
                }
                None
            }
            (None, GameCommand::CastUntargeted { spell, payment }) => {
                let cast = self.execution.cast_on_turn(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    None,
                    payment,
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                if !next.execution.has_deferred_payment() {
                    facts.mana_payment = Some(payment);
                }
                None
            }
        };
        next.basis.revision = revision;
        if let Some(progress) = progress {
            next.accept_rule_progress(progress, &mut facts)?;
        }
        if let Some(progress) = mana_progress {
            next.accept_mana_progress(progress, &mut facts)?;
        }
        let mut next = next.drive(&mut facts)?;
        next.capture_face_down_reveals(&facts.rules_journal);
        next.finalize_checked_losses();
        Ok(NativeCandidate {
            base: CandidateBase::Advance(self.basis.clone()),
            next,
            facts,
        })
    }
    fn validate_basis(&self, basis: &GameBasis) -> Result<(), GameError> {
        if basis.game != self.basis.game {
            return Err(GameError::WrongGame);
        }
        if basis.context != self.basis.context {
            return Err(GameError::WrongContext);
        }
        if basis.epoch != self.basis.epoch {
            return Err(GameError::WrongEpoch);
        }
        if basis.revision != self.basis.revision {
            return Err(GameError::StaleRevision);
        }
        Ok(())
    }
    fn frame_matches_turn_work(&self, frame: &RuleFrame, work: TurnWork) -> bool {
        if work == TurnWork::DeclareAttackers
            && self.execution.has_attack_mana_child()
            && matches!(
                frame,
                RuleFrame::ManaColor(_) | RuleFrame::Damage(_) | RuleFrame::ZoneReplacement(_)
            )
        {
            return true;
        }
        match (frame, work) {
            (RuleFrame::DrawReplacement(_) | RuleFrame::ZoneReplacement(_), TurnWork::Draw) => {
                self.execution.draw_decision() == frame.effect_decision()
            }
            (RuleFrame::TransformOrder(_) | RuleFrame::UntapKeepTapped(_), TurnWork::Untap) => true,
            (
                RuleFrame::EntryChoice(_)
                | RuleFrame::LinkedReturnOrder(_)
                | RuleFrame::LinkedAuraChoice(_),
                _,
            ) => self.execution.has_pending_cost_or_linked(),
            (RuleFrame::ZoneReplacement(_), _) if self.execution.has_pending_cost_or_linked() => {
                true
            }
            (RuleFrame::ZoneReplacement(frame), TurnWork::Cleanup)
                if self.execution.has_check_replacement_work() =>
            {
                matches!(frame.cause(),crate::occurrence::EventCause::CleanupDiscard {boundary:CheckBoundary::Cleanup(position),..}|crate::occurrence::EventCause::StateBasedActions {boundary:CheckBoundary::Cleanup(position),..} if self.turn.as_ref().is_some_and(|turn|turn.position()==*position))
            }
            (RuleFrame::Checks(frame), TurnWork::Cleanup) => {
                matches!(frame.boundary(), CheckBoundary::Cleanup(position)
                    if self.turn.as_ref().is_some_and(|turn| turn.position() == position))
            }
            (RuleFrame::Combat(CombatDecision::Attackers(_)), TurnWork::DeclareAttackers)
            | (RuleFrame::Combat(CombatDecision::Blockers(_)), TurnWork::DeclareBlockers) => true,
            (
                RuleFrame::Combat(CombatDecision::DamageAssignments(frame)),
                TurnWork::CombatDamage { additional },
            ) => self.turn.as_ref().is_some_and(|turn| {
                frame.position() == turn.position()
                    && frame.position().additional_damage == additional
            }),
            (
                RuleFrame::Damage(_) | RuleFrame::ZoneReplacement(_) | RuleFrame::RevealedPiles(_),
                TurnWork::CombatDamage { additional },
            ) => {
                matches!(self.execution.original_damage_origin(), Some(DamageOrigin::Combat { position })
                    if self.turn.as_ref().is_some_and(|turn| position == turn.position() && position.additional_damage == additional))
            }
            _ => false,
        }
    }
    /// Adopt every child append before the next owning operation or scheduler work.
    fn activate_for_input(
        &mut self,
        actor: SeatId,
        request: crate::activated::ActivationRequest,
        payment: Option<ManaPayment>,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        self.activate_for_input_restricted(actor, request, payment, &[], facts)
    }
    fn activate_for_input_restricted(
        &mut self,
        actor: SeatId,
        request: crate::activated::ActivationRequest,
        payment: Option<ManaPayment>,
        restricted: &[crate::mana::RestrictedManaSpend],
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let activated = self.execution.activate_ability_on_turn_restricted(
            self.execution.revision(),
            actor,
            request,
            payment,
            restricted,
            self.turn.as_ref(),
        )?;
        let result = self.accept_execution(activated, facts);
        if !self.execution.has_deferred_payment() {
            facts.mana_payment = Some(result.payment);
            facts.ability_activation = Some(result.activation);
        }
        Ok(())
    }
    fn accept_execution<R>(
        &mut self,
        transition: RulesTransition<EffectWindow, R>,
        facts: &mut TransitionFacts,
    ) -> R {
        let (state, result, append) = transition.into_boxed_parts();
        facts.rules_journal.append(append);
        self.execution = state;
        result
    }
    fn accept_rule_progress(
        &mut self,
        progress: RuleProgress,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let (mut state, result, append) = progress.into_boxed_parts();
        facts.rules_journal.append(append);
        if let Some(record) = state.take_completed_land_play() {
            facts.land_play = Some(record);
        }
        if let Some((entropy, shuffle)) = state.take_completed_search_shuffle() {
            self.random = Some(entropy);
            facts.shuffle_journal.push(shuffle);
        }
        if let Some((payment, activation)) = state.take_completed_deferred_receipts() {
            facts.mana_payment = Some(payment);
            facts.ability_activation = activation;
        }
        match result {
            RuleResult::Complete {
                completion,
                checks,
                draws,
                journal,
                declarations,
                assignments,
                permanent_entry: entry,
            } => {
                facts.check_journal.append(checks);
                facts.draw_journal.append(draws);
                facts.damage_journal.append(journal);
                facts.combat_declarations.extend(declarations);
                facts.combat_assignments.extend(assignments);
                let schedule = match completion {
                    RuleCompletion::Checks(completed) => {
                        if entry.is_some() {
                            return Err(GameError::UnexpectedKernelPhase);
                        }
                        match completed.boundary() {
                            CheckBoundary::Priority
                                if state.take_end_turn_jump()
                                    && state.state_based_losses().is_empty() =>
                            {
                                Some(
                                    self.turn
                                        .as_ref()
                                        .ok_or(GameError::UnexpectedKernelPhase)?
                                        .jump_to_cleanup()?,
                                )
                            }
                            CheckBoundary::Priority => None,
                            CheckBoundary::Cleanup(position) => {
                                let turn =
                                    self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                                if turn.position() != position
                                    || turn.stage() != TurnStage::Work(TurnWork::Cleanup)
                                {
                                    return Err(GameError::UnexpectedKernelPhase);
                                }
                                if state.state_based_losses().is_empty() {
                                    Some(turn.complete_work(WorkCompleted::Cleanup {
                                        priority_required: completed.performed()
                                            || completed.waiting_triggers(),
                                    })?)
                                } else {
                                    None
                                }
                            }
                        }
                    }
                    RuleCompletion::Resolution | RuleCompletion::GameEnded => None,
                    RuleCompletion::ManaAbility
                    | RuleCompletion::LinkedReturn
                    | RuleCompletion::LandPlay
                    | RuleCompletion::ActivationCost => None,
                    RuleCompletion::TurnWork(work) => {
                        if entry.is_some() {
                            return Err(GameError::UnexpectedKernelPhase);
                        }
                        Some(
                            self.turn
                                .as_ref()
                                .ok_or(GameError::UnexpectedKernelPhase)?
                                .complete_work(work)?,
                        )
                    }
                };
                if entry.is_some() {
                    if facts.permanent_entry.is_some() {
                        return Err(GameError::UnexpectedKernelPhase);
                    }
                    facts.permanent_entry = entry;
                }
                self.execution = state;
                self.pending = None;
                if let Some(schedule) = schedule {
                    self.accept_schedule(schedule, facts)?;
                }
            }
            RuleResult::Pending {
                frame,
                checks,
                draws,
                journal,
                declarations,
                assignments,
            } => {
                facts.check_journal.append(checks);
                facts.draw_journal.append(draws);
                facts.damage_journal.append(journal);
                facts.combat_declarations.extend(declarations);
                facts.combat_assignments.extend(assignments);
                if state.graveyard_shuffle_pending() {
                    let empty = EntropyTape::new(vec![]);
                    state.preflight_graveyard_shuffle(self.random.as_ref().unwrap_or(&empty))?;
                    if state.graveyard_shuffle_ready() {
                        let (next, entropy, shuffle) = state
                            .finish_graveyard_shuffle(self.random.as_ref().unwrap_or(&empty))?;
                        self.random = Some(entropy);
                        facts.shuffle_journal.push(shuffle);
                        return self.accept_rule_progress(next, facts);
                    }
                }
                if state.random_resolution_pending() {
                    let empty = EntropyTape::new(vec![]);
                    let (next, entropy) =
                        state.finish_random_resolution(self.random.as_ref().unwrap_or(&empty))?;
                    self.random = Some(entropy);
                    return self.accept_rule_progress(next, facts);
                }
                self.bind_decision(frame.into())?;
                self.execution = state;
            }
        }
        Ok(())
    }
    fn bind_decision(&mut self, frame: RuleFrame) -> Result<(), GameError> {
        if self.conclusion.is_some() {
            return Err(GameError::GameEnded);
        }
        let id = DecisionId(self.next_decision);
        self.next_decision = self
            .next_decision
            .checked_add(1)
            .ok_or(GameError::DecisionIdentityExhausted)?;
        self.pending = Some(DecisionState {
            id,
            basis: self.basis.clone(),
            chooser: frame.chooser(),
            frame,
        });
        Ok(())
    }
    fn accept_mana_progress(
        &mut self,
        progress: ManaActivationProgress,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let (state, result, append) = progress.into_boxed_parts();
        facts.rules_journal.append(append);
        match result {
            ManaActivationResult::Complete { production } => {
                self.execution = state;
                self.pending = None;
                facts.mana_production = Some(production);
            }
            ManaActivationResult::Pending { frame } => {
                self.bind_decision(RuleFrame::ManaColor(frame))?;
                self.execution = state;
            }
            ManaActivationResult::PendingRule { frame } => {
                self.bind_decision(frame.into())?;
                self.execution = state;
            }
        }
        Ok(())
    }
    fn drive(mut self, facts: &mut TransitionFacts) -> Result<Self, GameError> {
        if self.conclusion.is_some() {
            return Err(GameError::GameEnded);
        }
        loop {
            if self.finalize_effect_win(facts) {
                return Ok(self);
            }
            if self.pending.is_some() || !self.execution.state_based_losses().is_empty() {
                self.finalize_checked_losses();
                return Ok(self);
            }
            if let Some(frame) = self.execution.linked_return_decision()? {
                self.bind_decision(frame.into())?;
                return Ok(self);
            }
            if let Some(frame) = self.execution.pending_cast_cost_decision() {
                self.bind_decision(frame.into())?;
                return Ok(self);
            }
            if let Some(frame) = self.execution.pending_resolution_decision() {
                if self.execution.graveyard_shuffle_pending() {
                    let empty = EntropyTape::new(vec![]);
                    self.execution
                        .preflight_graveyard_shuffle(self.random.as_ref().unwrap_or(&empty))?;
                    if self.execution.graveyard_shuffle_ready() {
                        let (next, entropy, shuffle) = self
                            .execution
                            .clone()
                            .finish_graveyard_shuffle(self.random.as_ref().unwrap_or(&empty))?;
                        self.random = Some(entropy);
                        facts.shuffle_journal.push(shuffle);
                        self.accept_rule_progress(next, facts)?;
                        continue;
                    }
                }
                if self.execution.random_resolution_pending() {
                    let empty = EntropyTape::new(vec![]);
                    let (next, entropy) = self
                        .execution
                        .clone()
                        .finish_random_resolution(self.random.as_ref().unwrap_or(&empty))?;
                    self.random = Some(entropy);
                    self.accept_rule_progress(next, facts)?;
                    continue;
                }
                self.bind_decision(frame.into())?;
                return Ok(self);
            }
            if self
                .opening
                .as_ref()
                .is_some_and(|opening| !opening.complete())
            {
                self.drive_opening(facts)?;
                continue;
            }
            match self.execution.window().phase() {
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. }) => {
                    let progress = self.execution.begin_checks(self.execution.revision())?;
                    self.accept_rule_progress(progress, facts)?;
                }
                WindowPhase::CheckingTargets(_) | WindowPhase::Resolving(_) => {
                    let progress = self.execution.begin_resolution(self.execution.revision())?;
                    self.accept_rule_progress(progress, facts)?;
                }
                WindowPhase::StepEnded if self.turn.is_some() => {
                    let turn = self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                    let completed = if turn.position().step == Step::DeclareAttackers {
                        let combat = self
                            .execution
                            .combat()
                            .ok_or(GameError::UnexpectedKernelPhase)?;
                        PriorityCompleted::Attackers {
                            declared_or_put_attacking: combat.had_attackers(),
                        }
                    } else {
                        PriorityCompleted::Ordinary
                    };
                    let active_player = self.execution.window().active_player();
                    let skip_draw = turn.position().step == Step::Upkeep
                        && self.execution.should_skip_step(active_player, Step::Draw)?;
                    let mut skip_err: Option<crate::effects::EffectError> = None;
                    let mut consumed_skips: Vec<Step> = Vec::new();
                    if skip_draw {
                        consumed_skips.push(Step::Draw);
                    }
                    let mut next =
                        turn.complete_priority_with_step_skip(completed, skip_draw, |candidate| {
                            match self.execution.should_skip_step(active_player, candidate) {
                                Ok(skip) => {
                                    if skip {
                                        consumed_skips.push(candidate);
                                    }
                                    skip
                                }
                                Err(err) => {
                                    if skip_err.is_none() {
                                        skip_err = Some(err);
                                    }
                                    false
                                }
                            }
                        })?;
                    if let Some(err) = skip_err {
                        return Err(err.into());
                    }
                    for skipped in consumed_skips {
                        self.execution
                            .consume_skipped_step(active_player, skipped)?;
                    }
                    if matches!(turn.position().step, Step::EndCombat | Step::PostcombatMain)
                        && self.execution.take_additional_combat()
                    {
                        next = next.begin_additional_combat();
                    }

                    self.accept_schedule(next, facts)?;
                }
                WindowPhase::TurnWork => {
                    let Some(work) = self.pending_turn_work() else {
                        return Err(GameError::UnexpectedKernelPhase);
                    };
                    let result = match work {
                        TurnWork::Untap => {
                            if let Some(frame) = self.execution.next_transform_order()? {
                                self.bind_decision(RuleFrame::TransformOrder(frame))?;
                                return Ok(self);
                            }
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let transition = self
                                .execution
                                .untap_on_turn_with_choice(self.execution.revision(), turn)?;
                            if let Some(frame) = self.accept_execution(transition, facts) {
                                self.bind_decision(RuleFrame::UntapKeepTapped(frame))?;
                                return Ok(self);
                            }
                            WorkCompleted::Untap
                        }
                        TurnWork::Draw
                            if self.execution.window().active_participant().is_none() =>
                        {
                            WorkCompleted::Draw
                        }
                        TurnWork::Draw => {
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let transition = self.execution.progress_direct_draw(
                                self.execution.revision(),
                                self.execution.window().active_player(),
                                1.into(),
                                DrawOrigin::Turn {
                                    position: turn.position(),
                                },
                            )?;
                            match self.accept_execution(transition, facts) {
                                Ok(journal) => facts.draw_journal.append(journal),
                                Err(frame) => {
                                    facts
                                        .draw_journal
                                        .append(self.execution.take_draw_receipts());
                                    self.bind_decision(frame.into())?;
                                    return Ok(self);
                                }
                            }
                            if self.execution.won_by_effect.is_some() {
                                continue;
                            }
                            WorkCompleted::Draw
                        }
                        TurnWork::Cleanup => {
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let progress = self
                                .execution
                                .begin_cleanup(self.execution.revision(), turn)?;
                            self.accept_rule_progress(progress, facts)?;
                            continue;
                        }
                        TurnWork::DeclareAttackers | TurnWork::DeclareBlockers => {
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let progress = if work == TurnWork::DeclareAttackers {
                                self.execution
                                    .begin_declare_attackers(self.execution.revision(), turn)?
                            } else {
                                if self.execution.combat().is_none() {
                                    return Ok(self);
                                }
                                self.execution
                                    .begin_declare_blockers(self.execution.revision(), turn)?
                            };
                            self.accept_rule_progress(progress, facts)?;
                            continue;
                        }
                        TurnWork::CombatDamage { .. } => {
                            if self.execution.combat().is_none() {
                                return Ok(self);
                            }
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let progress = self
                                .execution
                                .begin_combat_damage(self.execution.revision(), turn)?;
                            self.accept_rule_progress(progress, facts)?;
                            continue;
                        }
                    };
                    let next = self
                        .turn
                        .as_ref()
                        .ok_or(GameError::UnexpectedKernelPhase)?
                        .complete_work(result)?;
                    self.accept_schedule(next, facts)?;
                }
                WindowPhase::Priority(PriorityPhase::Ready { .. }) | WindowPhase::StepEnded => {
                    return Ok(self);
                }
                _ => return Err(GameError::UnexpectedKernelPhase),
            }
        }
    }
    fn accept_schedule(
        &mut self,
        next: TurnScheduler,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let previous_turn = self
            .turn
            .as_ref()
            .ok_or(GameError::UnexpectedKernelPhase)?
            .clone();
        let previous = previous_turn.position();
        let position = next.position();
        let before = self.execution.window().active_player();
        let after = if previous.number != position.number {
            self.execution.take_next_active_player()?
        } else {
            before
        };
        if (previous.step == Step::EndCombat
            || previous.step.phase() == crate::turn::Phase::Combat
                && position.step == Step::Cleanup)
            && previous != position
        {
            let transition = self
                .execution
                .finish_combat_on_turn(self.execution.revision(), &previous_turn)?;
            self.accept_execution(transition, facts);
        }
        let transition = self.execution.enter_calendar(
            self.execution.revision(),
            Some(&previous_turn),
            &next,
            after,
        )?;
        self.accept_execution(transition, facts);
        if previous != position {
            facts.turn_transitions.push(TurnTransition {
                from: Some(previous),
                to: position,
                active_before: before,
                active_after: after,
            });
        }
        self.turn = Some(next);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activated::{ActivatedAbility, SourceCost};
    #[test]
    fn draw_failures_after_step_or_card_work_preserve_the_entire_accepted_input() {
        for fault in 0..5 {
            let mut state = GameState::initialize_scheduled(
                GameId(990),
                GameContext::new(
                    "r".into(),
                    "qualification-synthetic".into(),
                    "c".into(),
                    "p".into(),
                    "draw".into(),
                    crate::test_support::creature_types::synthetic_vocabulary().id(),
                    spellwake_contracts::names::NameRules::literal_names().id(),
                )
                .unwrap(),
                GameSeed::with_uniform_life(
                    vec![SeatId(0), SeatId(1)],
                    SeatId(0),
                    vec![SeedCard {
                        card: crate::object_identity::CardId(700),
                        owner: SeatId(0),
                        destination: crate::object_identity::ObjectDestination::LibraryBottom,
                        program: crate::effects::Program::creature(
                            None,
                            1.into(),
                            1.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    }],
                    vec![],
                    vec![FloatingMana {
                        player: SeatId(0),
                        mana_type: crate::mana::ManaType::Blue,
                        amount: 4.into(),
                    }],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    crate::test_support::creature_types::synthetic_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                ),
                TurnStart {
                    number: 2,
                    step: crate::turn::Step::Upkeep,
                },
            )
            .unwrap()
            .into_state();
            state = state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(0),
                    command: GameCommand::Pass,
                })
                .unwrap()
                .into_state();
            match fault {
                0 => state.basis.revision = u64::MAX,
                1 => state.execution.set_test_revision(u64::MAX - 3),
                2 => state.execution.set_test_window_revision(u64::MAX - 3),
                3 => state.execution.set_test_object_allocator(u64::MAX, false),
                _ => state.execution.set_test_object_allocator(u64::MAX, true),
            }
            let before = state.clone();
            assert!(
                state
                    .advance(GameInput {
                        basis: state.basis.clone(),
                        actor: SeatId(1),
                        command: GameCommand::Pass
                    })
                    .is_err()
            );
            assert_eq!(state, before);
            assert_eq!(state.execution.window().cards().library(SeatId(0)).len(), 1);
            assert_eq!(
                state.execution.mana(SeatId(0), crate::mana::ManaType::Blue),
                4.into()
            );
        }
    }
    fn failed_draw_fixture(both: bool, cleanup: bool) -> GameState {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut execution = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![],
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        execution = execution
            .enter_turn_step(execution.revision(), a, false, false)
            .unwrap()
            .into_state();
        execution = execution
            .draw_cards(execution.revision(), a, 1.into())
            .unwrap()
            .into_state();
        if both {
            execution = execution
                .draw_cards(execution.revision(), b, 1.into())
                .unwrap()
                .into_state();
        }
        if !cleanup {
            execution = execution
                .enter_turn_step(execution.revision(), a, true, false)
                .unwrap()
                .into_state();
        }
        GameState {
            basis: GameBasis {
                game: GameId(991),
                context: GameContext::new(
                    "r".into(),
                    "qualification-synthetic".into(),
                    "c".into(),
                    "p".into(),
                    "draw".into(),
                    crate::test_support::creature_types::synthetic_vocabulary().id(),
                    spellwake_contracts::names::NameRules::literal_names().id(),
                )
                .unwrap(),
                epoch: 0,
                revision: 0,
            },
            execution: Box::new(execution),
            search_reveal: None,
            face_down_reveals: Vec::new(),
            library_reveals: Vec::new(),
            cascade_reveals: Vec::new(),
            pending: None,
            counter_replacement: None,
            next_decision: 1,
            opening: None,
            random: None,
            attack_mana: None,
            conclusion: None,
            turn: if cleanup {
                Some(
                    TurnScheduler::new(
                        TurnStart {
                            number: 3,
                            step: crate::turn::Step::Cleanup,
                        },
                        true,
                    )
                    .unwrap(),
                )
            } else {
                None
            },
        }
    }
    #[test]
    fn simultaneous_empty_draw_losses_make_a_two_player_draw() {
        let state = failed_draw_fixture(true, false)
            .drive(&mut TransitionFacts::default())
            .unwrap();
        assert_eq!(state.outcome(), Some(GameOutcome::Draw));
        assert_eq!(state.execution.state_based_losses().len(), 2);
    }
    #[test]
    fn cleanup_loss_stops_before_turn_rotation_or_priority() {
        let mut facts = TransitionFacts::default();
        let state = failed_draw_fixture(false, true).drive(&mut facts).unwrap();
        assert_eq!(state.outcome(), Some(GameOutcome::Winner(SeatId(1))));
        assert_eq!(
            state.turn().unwrap().position().step,
            crate::turn::Step::Cleanup
        );
        assert!(facts.turn_transitions.is_empty());
        assert_eq!(state.execution.window().active_player(), SeatId(0));
        assert_eq!(state.execution.window().phase(), WindowPhase::TurnWork);
    }

    fn scheduled_end(number: u64) -> GameState {
        GameState::initialize_scheduled(
            GameId(700),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "c".into(),
                "p".into(),
                "turn".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![SeatId(0), SeatId(1)],
                SeatId(0),
                vec![],
                vec![],
                vec![FloatingMana {
                    player: SeatId(0),
                    mana_type: crate::mana::ManaType::Blue,
                    amount: 2.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            TurnStart {
                number,
                step: crate::turn::Step::End,
            },
        )
        .unwrap()
        .into_state()
    }

    #[test]
    fn late_step_failure_preserves_accepted_pass_state_and_nonzero_pools() {
        let initial = scheduled_end(2);
        let mut state = initial
            .advance(GameInput {
                basis: initial.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        state.execution.set_test_revision(u64::MAX - 1);
        let before = state.clone();
        assert!(
            state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(1),
                    command: GameCommand::Pass
                })
                .is_err()
        );
        assert_eq!(state, before);
        assert_eq!(
            state.execution.mana(SeatId(0), crate::mana::ManaType::Blue),
            2.into()
        );
    }

    #[test]
    fn late_turn_exhaustion_rolls_back_cleanup_and_mana_emptying() {
        let initial = scheduled_end(u64::MAX);
        let state = initial
            .advance(GameInput {
                basis: initial.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        let before = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: SeatId(1),
                command: GameCommand::Pass
            }),
            Err(GameError::Turn(TurnError::TurnExhausted))
        );
        assert_eq!(state, before);
        assert_eq!(
            state.execution.mana(SeatId(0), crate::mana::ManaType::Blue),
            2.into()
        );
    }
    fn activation_game(source_cost: SourceCost) -> GameState {
        use crate::mana::ManaType;
        let a = SeatId(0);
        let b = SeatId(1);
        GameState::initialize(
            GameId(63),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "activation".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    SeedCard {
                        card: CardId(1),
                        owner: b,
                        destination: ObjectDestination::Battlefield { controller: a },
                        program: Program::artifact(
                            None,
                            vec![ActivatedAbility {
                                id: ActivatedAbilityId(7),
                                mana_cost: crate::mana::ManaCost::one(ManaType::Red),
                                source_cost,
                                body: ExecutableBody::new(
                                    BodyTarget::SingleCreature,
                                    vec![Instruction::Damage { amount: 1.into() }],
                                ),
                            }],
                        ),
                    },
                    SeedCard {
                        card: CardId(2),
                        owner: b,
                        destination: ObjectDestination::Battlefield { controller: b },
                        program: Program::creature(
                            None,
                            3.into(),
                            5.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    },
                ],
                vec![CardId(1), CardId(2)],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 7.into(),
                    },
                    FloatingMana {
                        player: b,
                        mana_type: ManaType::Blue,
                        amount: 11.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state()
    }
    fn activation_input(state: &GameState) -> GameInput {
        GameInput {
            basis: state.basis.clone(),
            actor: SeatId(0),
            command: GameCommand::ActivateAbility {
                source: state
                    .execution
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap(),
                ability: ActivatedAbilityId(7),
                target: TargetRef::Object(
                    state
                        .execution
                        .window()
                        .cards()
                        .current_identity(CardId(2))
                        .unwrap(),
                ),
                payment: None,
            },
        }
    }
    #[test]
    fn activated_admission_late_failures_accept_no_cost_payment_or_entry() {
        use crate::mana::ManaType;
        use crate::stack_window::StackWindowError;
        for cost in [SourceCost::Tap, SourceCost::Sacrifice] {
            for fault in 0..5 {
                let mut state = activation_game(cost);
                let source = state
                    .execution
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap();
                let error = match fault {
                    0 => {
                        state.basis.revision = u64::MAX;
                        GameError::RevisionExhausted
                    }
                    1 => {
                        state.execution.set_test_revision(u64::MAX);
                        GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                    }
                    2 => {
                        state.execution.set_test_revision(u64::MAX - 1);
                        GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                    }
                    3 => {
                        state.execution.set_test_window_revision(u64::MAX - 1);
                        GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                    }
                    _ => {
                        state.execution.set_test_priority_revision(u64::MAX - 1);
                        GameError::Effect(EffectError::Window(StackWindowError::Priority(
                            crate::priority::PriorityError::RevisionExhausted,
                        )))
                    }
                };
                let before = state.clone();
                assert_eq!(state.advance(activation_input(&state)), Err(error));
                assert_eq!(state, before);
                assert_eq!(
                    state
                        .execution
                        .window()
                        .cards()
                        .get(source)
                        .unwrap()
                        .tapped(),
                    Some(false)
                );
                assert!(state.execution.window().entries().is_empty());
                assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 7.into());
                assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
            }
        }
    }
    #[test]
    fn activated_resolution_failure_keeps_previously_paid_sacrifice_and_stack() {
        use crate::mana::ManaType;
        let initial = activation_game(SourceCost::Sacrifice);
        let paid = initial
            .advance(activation_input(&initial))
            .unwrap()
            .into_state();
        let paid = paid
            .advance(GameInput {
                basis: paid.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        for remaining in [1, 2] {
            let mut state = paid.clone();
            state.execution.set_test_revision(u64::MAX - remaining);
            let before = state.clone();
            let source = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap();
            let target = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(2))
                .unwrap();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(1),
                    command: GameCommand::Pass
                }),
                Err(GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted
                )))
            );
            assert_eq!(state, before);
            assert_eq!(
                state.execution.window().cards().get(source).unwrap().zone(),
                crate::object_identity::ObjectZone::Graveyard(SeatId(1))
            );
            assert_eq!(
                state.execution.creature(target).unwrap().unwrap().damage,
                0.into()
            );
            assert_eq!(state.execution.window().entries().len(), 1);
            assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 6.into());
            assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
        }
    }
    fn color_game() -> GameState {
        use crate::mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType};
        use std::collections::BTreeSet;
        let a = SeatId(0);
        let b = SeatId(1);
        GameState::initialize(
            GameId(62),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "color".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![SeedCard {
                    card: CardId(1),
                    owner: b,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::land(
                        std::collections::BTreeSet::new(),
                        vec![ManaAbility {
                            id: ManaAbilityId(7),
                            output: ManaOutput::ChooseColor {
                                amount: 3.into(),
                                colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
                            },
                        }],
                    ),
                }],
                vec![CardId(1)],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 5.into(),
                    },
                    FloatingMana {
                        player: b,
                        mana_type: ManaType::Blue,
                        amount: 11.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state()
    }
    fn activate_color_input(state: &GameState) -> GameInput {
        GameInput {
            basis: state.basis.clone(),
            actor: SeatId(0),
            command: GameCommand::ActivateMana {
                source: state
                    .execution
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap(),
                ability: crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            },
        }
    }
    #[test]
    fn color_pending_admission_exhaustion_preserves_unpaid_state_and_basis() {
        use crate::mana::ManaType;
        for fault in 0..3 {
            let mut state = color_game();
            let source = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap();
            let error = match fault {
                0 => {
                    state.basis.revision = u64::MAX;
                    GameError::RevisionExhausted
                }
                1 => {
                    state.execution.set_test_revision(u64::MAX);
                    GameError::Effect(EffectError::Window(
                        crate::stack_window::StackWindowError::RevisionExhausted,
                    ))
                }
                _ => {
                    state.next_decision = u64::MAX;
                    GameError::DecisionIdentityExhausted
                }
            };
            let before = state.clone();
            assert_eq!(state.advance(activate_color_input(&state)), Err(error));
            assert_eq!(state, before);
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(false)
            );
            assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 5.into());
            assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
        }
        let mut state = color_game();
        state.next_decision = u64::MAX - 1;
        let candidate = state.advance(activate_color_input(&state)).unwrap();
        assert!(candidate.mana_production().is_none());
        assert!(candidate.mana_payment().is_none());
        let paid = candidate.into_state();
        let decision = paid.pending_decision().unwrap();
        assert_eq!(decision.id(), DecisionId(u64::MAX - 1));
        assert_eq!(decision.basis(), paid.basis());
        let complete = paid
            .advance(GameInput {
                basis: paid.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::ManaColor(ManaType::Green),
                },
            })
            .unwrap();
        assert!(complete.state().pending_decision().is_none());
        assert_eq!(
            complete
                .mana_production()
                .unwrap()
                .produced
                .get(&ManaType::Green),
            Some(&3.into())
        );
    }
    #[test]
    fn color_final_reply_failures_preserve_committed_tap_without_credit_or_record() {
        use crate::mana::ManaType;
        use crate::stack_window::StackWindowError;
        let initial = color_game();
        let initial = initial
            .advance(activate_color_input(&initial))
            .unwrap()
            .into_state();
        for fault in 0..5 {
            let mut state = initial.clone();
            let error = match fault {
                0 => {
                    state.basis.revision = u64::MAX;
                    GameError::RevisionExhausted
                }
                1 | 2 => {
                    let revision = if fault == 1 { u64::MAX } else { u64::MAX - 1 };
                    let frame = state.execution.set_test_mana_revision(revision);
                    state.pending.as_mut().unwrap().frame = RuleFrame::ManaColor(frame);
                    GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                }
                3 => {
                    state.execution.set_test_window_revision(u64::MAX);
                    GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                }
                _ => {
                    state.execution.set_test_priority_revision(u64::MAX);
                    GameError::Effect(EffectError::Window(StackWindowError::Priority(
                        crate::priority::PriorityError::RevisionExhausted,
                    )))
                }
            };
            let source = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap();
            let before = state.clone();
            let decision = state.pending_decision().unwrap().id();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(0),
                    command: GameCommand::Reply {
                        decision,
                        choice: DecisionReply::ManaColor(ManaType::Red)
                    }
                }),
                Err(error)
            );
            assert_eq!(state, before);
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(true)
            );
            assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 5.into());
            assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
        }
    }
    #[test]
    fn mana_activation_late_drive_failure_accepts_no_tap_credit_or_record() {
        use crate::mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType};
        use std::collections::BTreeMap;
        let a = SeatId(0);
        let b = SeatId(1);
        let initial = GameState::initialize(
            GameId(60),
            GameContext::new(
                "test".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::land(
                        std::collections::BTreeSet::new(),
                        vec![ManaAbility {
                            id: ManaAbilityId(1),
                            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 2.into())])),
                        }],
                    ),
                }],
                vec![CardId(1)],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 5.into(),
                    },
                    FloatingMana {
                        player: b,
                        mana_type: ManaType::Blue,
                        amount: 11.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        let source = initial
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for fault in 0..4 {
            let mut state = initial.clone();
            let expected = if fault == 0 {
                state.basis.revision = u64::MAX;
                GameError::RevisionExhausted
            } else {
                match fault {
                    1 => state.execution.set_test_revision(u64::MAX),
                    2 => state.execution.set_test_revision(u64::MAX - 1),
                    _ => state.execution.set_test_window_revision(u64::MAX - 1),
                }
                GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted,
                ))
            };
            let before = state.clone();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: a,
                    command: GameCommand::ActivateMana {
                        source,
                        ability: crate::mana::ManaAbilityRef::Printed(ManaAbilityId(1))
                    }
                }),
                Err(expected)
            );
            assert_eq!(state, before);
            assert_eq!(state.execution.mana(a, ManaType::Red), 5.into());
            assert_eq!(state.execution.mana(b, ManaType::Blue), 11.into());
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(false)
            );
            assert!(state.execution.window().entries().is_empty());
        }
    }
    use crate::{
        effects::{Instruction, Program},
        mana::{ManaCost, ManaType},
        object_identity::{CardId, ObjectDestination},
    };
    #[test]
    fn paid_cast_outer_and_post_cast_drive_overflow_accept_no_receipt_or_debit() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = GameState::initialize(
            GameId(50),
            GameContext::new(
                "test".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    SeedCard {
                        card: CardId(1),
                        owner: a,
                        destination: ObjectDestination::Hand,
                        program: Program::instant(
                            ManaCost::one(ManaType::Red),
                            ExecutableBody::new(
                                BodyTarget::SingleCreature,
                                vec![Instruction::Damage { amount: 1.into() }],
                            ),
                        ),
                    },
                    SeedCard {
                        card: CardId(2),
                        owner: b,
                        destination: ObjectDestination::Battlefield { controller: b },
                        program: Program::creature(
                            None,
                            1.into(),
                            10.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    },
                ],
                vec![CardId(2)],
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        let spell = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        let target = TargetRef::Object(
            state
                .execution
                .window()
                .cards()
                .current_identity(CardId(2))
                .unwrap(),
        );
        state.basis.revision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: a,
                command: GameCommand::Cast { spell, target }
            }),
            Err(GameError::RevisionExhausted)
        );
        assert_eq!(state, saved);
        state.basis.revision = 0;
        for revision in [u64::MAX, u64::MAX - 1] {
            state.execution.set_test_revision(revision);
            let saved = state.clone();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: a,
                    command: GameCommand::Cast { spell, target }
                }),
                Err(GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted
                )))
            );
            assert_eq!(state, saved);
            assert_eq!(state.execution.mana(a, ManaType::Red), 2.into());
            assert!(state.execution.window().entries().is_empty());
        }
    }
    #[test]
    fn initial_program_allocation_failure_discards_all_prior_instructions() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = GameState::initialize(
            GameId(40),
            GameContext::new(
                "test".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    SeedCard {
                        card: CardId(1),
                        owner: a,
                        destination: ObjectDestination::Hand,
                        program: Program::instant(
                            ManaCost::one(ManaType::Red),
                            ExecutableBody::new(
                                BodyTarget::SingleCreature,
                                vec![
                                    Instruction::Damage { amount: 2.into() },
                                    Instruction::CreatePreventionShield { amount: 3.into() },
                                    Instruction::CreatePreventionShield { amount: 4.into() },
                                    Instruction::Damage { amount: 5.into() },
                                ],
                            ),
                        ),
                    },
                    SeedCard {
                        card: CardId(2),
                        owner: b,
                        destination: ObjectDestination::Battlefield { controller: b },
                        program: Program::creature(
                            None,
                            1.into(),
                            100.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    },
                ],
                vec![CardId(2)],
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        let target = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(2))
            .unwrap();
        let spell = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for (actor, command) in [
            (
                a,
                GameCommand::Cast {
                    spell,
                    target: TargetRef::Object(target),
                },
            ),
            (a, GameCommand::Pass),
        ] {
            state = state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        state.next_decision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: b,
                command: GameCommand::Pass
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(state, saved);
        assert_eq!(
            state.execution.creature(target).unwrap().unwrap().damage,
            0.into()
        );
        assert!(state.execution.prevention_shields().next().is_none());
        assert_eq!(state.execution.instruction_index(), None);
        assert!(state.pending.is_none());
    }
    #[test]
    fn allocation_failure_after_a_mixed_reply_preserves_accepted_suspension() {
        let a = SeatId(0);
        let b = SeatId(1);
        let instructions = [
            Instruction::CreatePreventionShield { amount: 1.into() },
            Instruction::CreateDamageMultiplier { factor: 2.into() },
            Instruction::CreateDamageMultiplier { factor: 3.into() },
            Instruction::Damage { amount: 5.into() },
        ];
        let mut cards: Vec<_> = instructions
            .into_iter()
            .enumerate()
            .map(|(index, instruction)| SeedCard {
                card: CardId(index as u64 + 1),
                owner: a,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost::one(ManaType::Red),
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
                ),
            })
            .collect();
        cards.push(SeedCard {
            card: CardId(10),
            owner: b,
            destination: ObjectDestination::Battlefield { controller: b },
            program: Program::creature(
                None,
                1.into(),
                100.into(),
                std::collections::BTreeSet::new(),
            ),
        });
        let context = GameContext::new(
            "test".into(),
            "qualification-synthetic".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
            crate::test_support::creature_types::synthetic_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap();
        let mut state = GameState::initialize(GameId(20), context, {
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        crate::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                cards,
                initial_battlefield_order,
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 4.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        })
        .unwrap()
        .into_state();
        for card in 1..=4 {
            let target = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(10))
                .unwrap();
            let spell = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(card))
                .unwrap();
            for (actor, command) in [
                (
                    a,
                    GameCommand::Cast {
                        spell,
                        target: TargetRef::Object(target),
                    },
                ),
                (a, GameCommand::Pass),
                (b, GameCommand::Pass),
            ] {
                state = state
                    .advance(GameInput {
                        basis: state.basis.clone(),
                        actor,
                        command,
                    })
                    .unwrap()
                    .into_state();
            }
        }
        assert!(state.pending.is_some());
        state.next_decision = u64::MAX;
        let saved = state.clone();
        let shield = state.execution.prevention_shields().next().unwrap().id;
        let pending = state.pending.as_ref().unwrap();
        let RuleFrame::Damage(DamageDecision::Order(frame)) = pending.frame() else {
            panic!("damage ordering expected")
        };
        let application = frame
            .applications()
            .iter()
            .find(|application| application.effect == shield.into())
            .copied()
            .unwrap();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: b,
                command: GameCommand::Reply {
                    decision: pending.id,
                    choice: DecisionReply::DamageOrder(application)
                }
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(state, saved);
        assert_eq!(
            state
                .execution
                .pending_damage()
                .unwrap()
                .components()
                .next()
                .unwrap()
                .remaining(),
            &5.into()
        );
        assert_eq!(
            state.execution.prevention_shield(shield).unwrap().remaining,
            1.into()
        );
        assert_eq!(state.execution.pending_damage().unwrap().stage(), 0);
    }
    #[test]
    fn context_rejects_missing_or_noncanonical_identity_components() {
        for index in 0..5 {
            for invalid in ["", "   ", " release", "release "] {
                let mut labels =
                    ["release", "rules", "catalogue", "programs", "profile"].map(String::from);
                labels[index] = invalid.into();
                let [release, rules, catalogue, programs, profile] = labels;
                assert_eq!(
                    GameContext::new(
                        release,
                        rules,
                        catalogue,
                        programs,
                        profile,
                        crate::test_support::creature_types::synthetic_vocabulary().id(),
                        spellwake_contracts::names::NameRules::literal_names().id(),
                    ),
                    Err(GameError::InvalidContext)
                );
            }
        }
    }
    #[test]
    fn revision_exhaustion_preserves_private_state_without_forging_a_public_constructor() {
        let context = GameContext::new(
            "r".into(),
            "qualification-synthetic".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
            crate::test_support::creature_types::synthetic_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap();
        let mut game = GameState::initialize(
            GameId(1),
            context,
            GameSeed::with_uniform_life(
                vec![SeatId(0), SeatId(1)],
                SeatId(0),
                vec![],
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        game.basis.revision = u64::MAX;
        let saved = game.clone();
        assert_eq!(
            game.advance(GameInput {
                basis: game.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass
            }),
            Err(GameError::RevisionExhausted)
        );
        assert_eq!(game, saved);
    }

    #[test]
    fn decision_identity_exhaustion_rejects_the_last_pass_without_partial_resolution() {
        let a = SeatId(0);
        let b = SeatId(1);
        let context = GameContext::new(
            "native-test".into(),
            "qualification-synthetic".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
            crate::test_support::creature_types::synthetic_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap();
        let instant = |card, owner, colour, instructions, body_target| SeedCard {
            card: CardId(card),
            owner,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(colour),
                ExecutableBody::new(body_target, instructions),
            ),
        };
        let mut game = GameState::initialize(
            GameId(1),
            context,
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    instant(
                        1,
                        a,
                        ManaType::Red,
                        vec![Instruction::Damage { amount: 3.into() }],
                        BodyTarget::SingleCreature,
                    ),
                    instant(
                        2,
                        b,
                        ManaType::Blue,
                        vec![Instruction::CopySpell, Instruction::CopySpell],
                        BodyTarget::SingleSpell(SpellFilter::copyable_instruction()),
                    ),
                    SeedCard {
                        card: CardId(3),
                        owner: b,
                        destination: ObjectDestination::Battlefield { controller: b },
                        program: Program::creature(
                            None,
                            2.into(),
                            2.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    },
                ],
                vec![CardId(3)],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
                        amount: 1.into(),
                    },
                    FloatingMana {
                        player: b,
                        mana_type: ManaType::Blue,
                        amount: 1.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        let object = |game: &GameState, card| {
            game.execution
                .window()
                .cards()
                .current_identity(CardId(card))
                .unwrap()
        };
        let command = GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Object(object(&game, 3)),
        };
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command,
            })
            .unwrap()
            .into_state();
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        let original = game.execution.window().entries()[0].id();
        let command = GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Stack(original),
        };
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: b,
                command,
            })
            .unwrap()
            .into_state();
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: b,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        game.next_decision = u64::MAX;
        let saved = game.clone();
        assert_eq!(
            game.advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(game, saved);
        assert!(game.pending_decision().is_none());
        assert!(game.execution.window().spell_copies().is_empty());
        assert!(
            matches!(game.execution.window().phase(), WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == a)
        );
        // Accept the first suspension, then fail only after its valid reply creates
        // a copy and the next instruction needs another decision identity.
        game.next_decision = 1;
        let mut pending = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        pending.next_decision = u64::MAX;
        let saved = pending.clone();
        let decision = pending.pending.as_ref().unwrap();
        assert_eq!(
            pending.advance(GameInput {
                basis: pending.basis.clone(),
                actor: b,
                command: GameCommand::Reply {
                    decision: decision.id,
                    choice: DecisionReply::CopyTargets(crate::effects::CopyTargetChoice::Retain)
                }
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(pending, saved);
        assert!(pending.execution.window().spell_copies().is_empty());
        assert_eq!(pending.execution.instruction_index(), Some(0));
    }
}

#[cfg(test)]
mod combat_tests {
    use super::*;
    use crate::{
        combat::{AttackDeclaration, BlockDeclaration, BlockStatus},
        effects::{Instruction, Program},
        mana::ManaCost,
        object_identity::{CardId, ObjectDestination},
    };
    const A: SeatId = SeatId(17);
    const B: SeatId = SeatId(4);
    const C: SeatId = SeatId(90);
    const D: SeatId = SeatId(2);

    fn fixture(multiplayer: bool) -> GameState {
        let mut cards = vec![];
        for (id, player) in [(1, A), (2, A), (3, B), (4, B), (5, C), (6, D), (7, A)] {
            if !multiplayer && [C, D].contains(&player) {
                continue;
            }
            cards.push(SeedCard {
                card: CardId(id),
                owner: player,
                destination: ObjectDestination::Battlefield { controller: player },
                program: Program::creature(
                    None,
                    2.into(),
                    4.into(),
                    std::collections::BTreeSet::new(),
                ),
            });
        }
        cards.push(SeedCard {
            card: CardId(30),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            ),
        });
        cards.push(SeedCard {
            card: CardId(100),
            owner: A,
            destination: ObjectDestination::LibraryBottom,
            program: Program::creature(None, 2.into(), 4.into(), std::collections::BTreeSet::new()),
        });
        let game = GameState::initialize_scheduled(
            GameId(1510),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "c".into(),
                "p".into(),
                "combat-fault".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            {
                // Fixture premise: seeded BF timestamps follow this authored card sequence.
                let initial_battlefield_order = cards
                    .iter()
                    .filter(|card| {
                        matches!(
                            card.destination,
                            crate::object_identity::ObjectDestination::Battlefield { .. }
                        )
                    })
                    .map(|card| card.card)
                    .collect();
                GameSeed::with_uniform_life(
                    if multiplayer {
                        vec![B, C, A, D]
                    } else {
                        vec![A, B]
                    },
                    A,
                    cards,
                    initial_battlefield_order,
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    crate::test_support::creature_types::synthetic_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            TurnStart::default(),
        )
        .unwrap()
        .into_state();
        let mut next = game;
        while next.turn().unwrap().position().step != Step::BeginningCombat {
            next = round(&next);
        }
        next
    }
    fn object(game: &GameState, id: u64) -> ObjectId {
        game.execution
            .window()
            .cards()
            .current_identity(CardId(id))
            .unwrap()
    }
    fn holder(game: &GameState) -> SeatId {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            game.execution.window().phase()
        else {
            panic!("priority expected")
        };
        holder
    }
    fn command(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
        GameInput {
            basis: game.basis.clone(),
            actor,
            command,
        }
    }
    fn pass(game: &GameState) -> GameState {
        game.advance(command(game, holder(game), GameCommand::Pass))
            .unwrap()
            .into_state()
    }
    fn round(game: &GameState) -> GameState {
        let mut next = game.clone();
        for _ in game.execution.window().participants() {
            next = pass(&next);
        }
        next
    }
    fn reply(game: &GameState, choice: DecisionReply) -> GameInput {
        let pending = game.pending.as_ref().unwrap();
        command(
            game,
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
        )
    }
    fn attack(game: &GameState, selected: &[(u64, SeatId)]) -> GameState {
        game.advance(reply(
            game,
            DecisionReply::DeclareAttackers(
                selected
                    .iter()
                    .map(|&(id, defender)| AttackDeclaration {
                        attacker: object(game, id),
                        defender,
                        defending_planeswalker: None,
                    })
                    .collect(),
            ),
        ))
        .unwrap()
        .into_state()
    }
    fn block(game: &GameState, blocker: u64, attacker: u64) -> GameState {
        game.advance(reply(
            game,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: object(game, blocker),
                attacker: object(game, attacker),
            }]),
        ))
        .unwrap()
        .into_state()
    }

    fn pending_damage_assignments() -> GameState {
        let ready = round(&fixture(false));
        let attacking = attack(&ready, &[(1, B), (2, B)]);
        let blocking = round(&attacking);
        let blocked = blocking
            .advance(reply(
                &blocking,
                DecisionReply::DeclareBlockers(vec![
                    BlockDeclaration {
                        blocker: object(&blocking, 3),
                        attacker: object(&blocking, 1),
                    },
                    BlockDeclaration {
                        blocker: object(&blocking, 4),
                        attacker: object(&blocking, 1),
                    },
                ]),
            ))
            .unwrap()
            .into_state();
        let pending = round(&blocked);
        assert!(matches!(
            pending.pending.as_ref().unwrap().frame(),
            RuleFrame::Combat(CombatDecision::DamageAssignments(_))
        ));
        pending
    }

    fn whole_actor_assignments(game: &GameState) -> Vec<crate::combat::CombatDamageAssignment> {
        use crate::{combat::CombatDamageAssignment, damage::DamageRecipient};
        vec![
            CombatDamageAssignment {
                source: object(game, 1),
                target: DamageRecipient::Creature(object(game, 3)),
                amount: 1.into(),
            },
            CombatDamageAssignment {
                source: object(game, 1),
                target: DamageRecipient::Creature(object(game, 4)),
                amount: 1.into(),
            },
            CombatDamageAssignment {
                source: object(game, 2),
                target: DamageRecipient::Player(B),
                amount: 2.into(),
            },
        ]
    }

    #[test]
    fn damage_assignment_reply_covers_forced_sources_and_commits_one_simultaneous_batch() {
        use crate::combat::{AssignmentOrigin, CombatError};
        let game = pending_damage_assignments();
        let saved = game.clone();
        let mut missing_forced = whole_actor_assignments(&game);
        missing_forced.pop();
        assert_eq!(
            game.advance(reply(
                &game,
                DecisionReply::AssignCombatDamage(missing_forced)
            )),
            Err(GameError::Effect(EffectError::Combat(
                CombatError::IncorrectDamageTotal
            )))
        );
        assert_eq!(game, saved);
        assert_eq!(game.execution.life_total(B), Some(&20.into()));
        assert!(game.execution.pending_damage().is_none());
        for id in [1, 2, 3, 4] {
            assert_eq!(
                game.execution
                    .creature(object(&game, id))
                    .unwrap()
                    .unwrap()
                    .damage,
                0.into()
            );
        }
        assert_eq!(
            game.advance(command(&game, A, GameCommand::Pass)),
            Err(GameError::TurnWorkRequired(TurnWork::CombatDamage {
                additional: false
            }))
        );
        let completed = game
            .advance(reply(
                &game,
                DecisionReply::AssignCombatDamage(whole_actor_assignments(&game)),
            ))
            .unwrap();
        assert_eq!(game, saved);
        assert_eq!(completed.damage_journal().batches.len(), 1);
        assert_eq!(completed.damage_journal().attempts.len(), 5);
        assert_eq!(completed.damage_journal().applied.len(), 5);
        assert_eq!(completed.combat_assignments().len(), 2);
        assert_eq!(completed.combat_assignments()[0].player, A);
        assert_eq!(
            completed.combat_assignments()[0].origin,
            AssignmentOrigin::PlayerReply
        );
        assert_eq!(completed.combat_assignments()[0].assignments.len(), 3);
        assert_eq!(completed.combat_assignments()[1].player, B);
        assert_eq!(
            completed.combat_assignments()[1].origin,
            AssignmentOrigin::Forced
        );
        let state = completed.state();
        assert_eq!(state.execution.life_total(B), Some(&18.into()));
        assert!(
            state
                .execution
                .creature(object(&game, 1))
                .unwrap()
                .is_none()
        );
        for id in [3, 4] {
            assert_eq!(
                state
                    .execution
                    .creature(object(state, id))
                    .unwrap()
                    .unwrap()
                    .damage,
                1.into()
            );
        }
        assert_eq!(state.turn().unwrap().position().step, Step::CombatDamage);
        assert_eq!(state.turn().unwrap().stage(), TurnStage::Priority);
        assert_eq!(holder(state), A);
        let passed = pass(state);
        assert_eq!(passed.turn().unwrap().position().step, Step::CombatDamage);
    }

    #[test]
    fn late_damage_schedule_or_check_failure_preserves_the_entire_pending_actor() {
        for revision in [u64::MAX, u64::MAX - 1] {
            let mut game = pending_damage_assignments();
            game.execution.set_test_window_revision(revision);
            let saved = game.clone();
            assert!(
                game.advance(reply(
                    &game,
                    DecisionReply::AssignCombatDamage(whole_actor_assignments(&game)),
                ))
                .is_err()
            );
            assert_eq!(game, saved);
            assert_eq!(game.execution.life_total(B), Some(&20.into()));
            assert!(game.execution.pending_damage().is_none());
            assert!(game.pending.is_some());
        }
    }

    #[test]
    fn late_attack_card_schedule_and_check_failures_preserve_pending_choice_and_all_cards() {
        for fault in 0..3 {
            let mut game = round(&fixture(false));
            match fault {
                0 => game
                    .execution
                    .set_test_object_allocator(u64::MAX - 1, false),
                1 => game.execution.set_test_window_revision(u64::MAX - 1),
                _ => game.execution.set_test_window_revision(u64::MAX - 2),
            }
            let incoming = reply(
                &game,
                DecisionReply::DeclareAttackers(vec![
                    AttackDeclaration {
                        attacker: object(&game, 1),
                        defender: B,
                        defending_planeswalker: None,
                    },
                    AttackDeclaration {
                        attacker: object(&game, 2),
                        defender: B,
                        defending_planeswalker: None,
                    },
                ]),
            );
            let saved = game.clone();
            assert!(game.advance(incoming).is_err(), "fault {fault}");
            assert_eq!(game, saved);
            assert!(game.pending.is_some());
            assert!(game.execution.combat().is_none());
            for id in [1, 2] {
                assert_eq!(
                    game.execution
                        .window()
                        .cards()
                        .get(object(&game, id))
                        .unwrap()
                        .tapped(),
                    Some(false)
                );
            }
        }
    }

    #[test]
    fn failure_to_bind_the_new_attacker_decision_preserves_the_last_priority_pass() {
        for token in [false, true] {
            let mut game = pass(&fixture(false));
            if token {
                game.next_decision = u64::MAX;
            } else {
                game.execution.set_test_revision(u64::MAX - 1);
            }
            let saved = game.clone();
            assert!(game.advance(command(&game, B, GameCommand::Pass)).is_err());
            assert_eq!(game, saved);
            assert_eq!(game.turn().unwrap().position().step, Step::BeginningCombat);
            assert_eq!(holder(&game), B);
            assert!(game.pending.is_none());
            assert!(game.execution.combat().is_none());
        }
    }

    #[test]
    fn later_blocker_cursor_allocation_failure_keeps_earlier_accepted_defender() {
        let ready = round(&fixture(true));
        let attacking = attack(&ready, &[(1, D), (2, B), (7, C)]);
        let first = round(&attacking);
        assert_eq!(first.pending.as_ref().unwrap().chooser(), D);
        let mut next = block(&first, 6, 1);
        assert_eq!(next.pending.as_ref().unwrap().chooser(), B);
        next.next_decision = u64::MAX;
        let saved = next.clone();
        assert_eq!(
            next.advance(reply(
                &next,
                DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    blocker: object(&next, 3),
                    attacker: object(&next, 2)
                }])
            )),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(next, saved);
        let combat = next.execution.combat().unwrap();
        assert!(combat.blocker(object(&next, 6)).is_some());
        assert!(combat.blocker(object(&next, 3)).is_none());
        assert_eq!(
            combat.attacker(object(&next, 1)).unwrap().block_status(),
            BlockStatus::Blocked
        );
        assert_eq!(
            combat.attacker(object(&next, 2)).unwrap().block_status(),
            BlockStatus::NotDeclared
        );
    }

    fn nonempty_end_combat() -> GameState {
        let ready = round(&fixture(false));
        let attacking = attack(&ready, &[(1, B)]);
        let blocking = round(&attacking);
        let blocked = block(&blocking, 3, 1);
        let damage = round(&blocked);
        assert_eq!(damage.turn().unwrap().position().step, Step::CombatDamage);
        assert_eq!(damage.turn().unwrap().stage(), TurnStage::Priority);
        assert_eq!(
            damage
                .execution
                .creature(object(&damage, 1))
                .unwrap()
                .unwrap()
                .damage,
            2.into()
        );
        assert_eq!(
            damage
                .execution
                .creature(object(&damage, 3))
                .unwrap()
                .unwrap()
                .damage,
            2.into()
        );
        let end = round(&damage);
        assert_eq!(end.turn().unwrap().position().step, Step::EndCombat);
        end
    }

    #[test]
    fn private_nonempty_context_clears_only_after_actual_end_combat_priority_exit() {
        let initial = nonempty_end_combat();
        let combat = initial.execution.combat().unwrap().clone();
        assert_eq!(combat.attackers().count(), 1);
        assert_eq!(combat.blockers().count(), 1);
        let cast = initial
            .advance(command(
                &initial,
                A,
                GameCommand::Cast {
                    spell: object(&initial, 30),
                    target: TargetRef::Object(object(&initial, 4)),
                },
            ))
            .unwrap()
            .into_state();
        let resolved = round(&cast);
        assert_eq!(resolved.turn().unwrap().position().step, Step::EndCombat);
        assert_eq!(resolved.execution.combat(), Some(&combat));
        let before = resolved.execution.window().cards().clone();
        let first_pass = pass(&resolved);
        assert_eq!(first_pass.execution.combat(), Some(&combat));
        let exited = pass(&first_pass);
        assert_eq!(exited.turn().unwrap().position().step, Step::PostcombatMain);
        assert!(exited.execution.combat().is_none());
        assert_eq!(exited.execution.window().cards(), &before);
        assert_eq!(
            exited
                .execution
                .window()
                .cards()
                .get(object(&exited, 1))
                .unwrap()
                .tapped(),
            Some(true)
        );
    }

    #[test]
    fn private_nonempty_exit_failure_does_not_publish_metadata_clearing() {
        let mut game = pass(&nonempty_end_combat());
        game.execution.set_test_revision(u64::MAX - 1);
        let saved = game.clone();
        assert!(game.advance(command(&game, B, GameCommand::Pass)).is_err());
        assert_eq!(game, saved);
        assert_eq!(game.execution.combat().unwrap().blockers().count(), 1);
        assert_eq!(game.turn().unwrap().position().step, Step::EndCombat);
    }
}

#[cfg(test)]
mod targeted_trigger_tests;

#[cfg(test)]
mod reference_trace;
#[cfg(test)]
mod targeted_reference_tests;

#[cfg(test)]
mod conditional_reference_tests;

#[cfg(test)]
mod conditional_history_tests;
