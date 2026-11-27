//! Universal composable `DrawCards` and `MutateObject` primitives.
//!
//! Replaces dozens of legacy draw, tap/untap, phase-out, transform, face-down/up,
//! regenerate, goad, exert, attach/unattach, counter manipulation, and control
//! macros with two orthogonal, query-driven primitives.

use crate::{
    characteristics::{Color, KeywordAbility, ResolvedDuration},
    effects::CounterKind,
    object_identity::ObjectId,
    program::{
        CardType, CombatTriggerKeyword, CounterKeyword, DefinitionError, GrowthKeyword, SplitHalf,
        TransferKeyword, ValueExpr,
    },
    toolbox::{
        env::BindingId,
        query::{ObjectQuery, PlayerSelector},
        zone_op::ZoneSelection,
    },
};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
};
use std::collections::BTreeSet;

/// Configuration for creating token copies of selected object(s).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenCopySpec {
    pub count: ValueExpr,
    pub recipients: crate::token::TokenRecipients,
    pub enters_tapped: bool,
    pub enters_attacking: bool,
    pub haste: bool,
    pub sacrifice_at_end_step: bool,
    pub exile_at_end_step: bool,
    pub nonlegendary: bool,
    pub extra_types: BTreeSet<CardType>,
    pub extra_subtypes: BTreeSet<CreatureTypeId>,
    pub extra_keywords: BTreeSet<KeywordAbility>,
    pub override_pt: Option<(RulesInteger, RulesInteger)>,
    pub extra_colors: BTreeSet<Color>,
    pub override_colors: Option<BTreeSet<Color>>,
    pub strip_mana_cost: bool,
    pub defender_scope: crate::token::AttackingDefenderScope,
    pub extra_activated: Vec<crate::activated::ActivatedAbility>,
    pub extra_triggered: Vec<crate::program::TriggeredAbility>,
    pub extra_static: Vec<crate::characteristics::StaticContinuousAbility>,
    pub extra_mana: Vec<crate::mana::ManaAbility>,
    pub extra_counters: Vec<(crate::effects::CounterKind, u8)>,
    pub keep_name: bool,
}

/// Universal card-draw specification (`players` each draw `amount` cards).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawCardsSpec {
    pub players: PlayerSelector,
    pub amount: ValueExpr,
    /// Optional binding receiving the drawn object IDs in [`crate::toolbox::ExecEnv`].
    pub bind_drawn: Option<BindingId>,
}

impl DrawCardsSpec {
    /// Construct a plain draw spec for `players` drawing `amount` cards.
    #[must_use]
    pub fn simple(players: PlayerSelector, amount: ValueExpr) -> Self {
        Self {
            players,
            amount,
            bind_drawn: None,
        }
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.players.uses_target() || self.amount.uses_target()
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.players.validate_vocabulary(vocabulary)?;
        self.amount
            .validate_vocabulary(vocabulary)
            .map_err(|_| DefinitionError::InvalidInstruction)
    }
}

/// Tap, untap, and exert mutations executed by [`ObjectMutation::TapState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapStateOp {
    /// Tap (`true`) or untap (`false`) each selected permanent.
    Set(bool),
    /// Flip tapped status of each selected permanent (tap if untapped, untap if tapped).
    Toggle,
    /// Mark each selected permanent not to untap during its controller's next untap step.
    SkipNextUntap,
    /// Exert each selected permanent (skips next untap step + emits Exert occurrence).
    Exert,
}

/// Phase, transform, and face-up/down mutations executed by [`ObjectMutation::Orientation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrientationOp {
    /// Phase out each selected permanent (CR 702.26).
    PhaseOut,
    /// Transform each selected double-faced permanent (CR 701.28).
    Transform,
    /// Turn each selected face-down permanent face up (CR 708).
    TurnFaceUp,
    /// Turn each selected face-up permanent face down as a 2/2 creature (CR 708).
    TurnFaceDown,
}

/// Attachment mutations executed by [`ObjectMutation::Attach`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachOp {
    /// Attach each selected attachment (Aura/Equipment/Fortification) to the first object in `host`.
    /// When `ability` is `Some`, enforces sorcery-speed Equip/Fortify/Reconfigure rules.
    ToObject {
        host: Box<ObjectQuery>,
        ability: Option<crate::attachment::AttachAbility>,
    },
    /// Attach each selected attachment to the first player in `player`.
    ToPlayer { player: PlayerSelector },
    /// Unattach each selected attachment from whatever it is attached to.
    Unattach,
}

/// Control-changing mutations executed by [`ObjectMutation::Control`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlOp {
    /// Change controller of each selected permanent to `controller` for `duration`.
    Gain {
        controller: PlayerSelector,
        duration: ResolvedDuration,
    },
    /// Exchange control of each selected permanent with the first permanent in `other` (CR 701.10a).
    Exchange { other: Box<ObjectQuery> },
}

/// Counter mutations executed by [`ObjectMutation::Counters`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CounterOp {
    /// Put `amount` counters of `kind` on each selected object.
    Add {
        kind: CounterKind,
        amount: ValueExpr,
    },
    /// Remove up to `amount` counters of `kind` (or any kind if `None`) from each selected object.
    Remove {
        kind: Option<CounterKind>,
        amount: ValueExpr,
    },
    /// Remove all counters of `kind` (or all counters if `None`) from each selected object.
    Clear { kind: Option<CounterKind> },
    /// Double all counters of `kind` (or every counter kind if `None`) on each selected object.
    Double { kind: Option<CounterKind> },
    /// Move counters from selected object(s) to the first object resolved by `to`.
    Move {
        to: ObjectQuery,
        kind: Option<CounterKind>,
        amount: Option<ValueExpr>,
    },
    /// Put one additional counter of each kind already on each selected object.
    AddOneOfEach,
    /// Copy all counters recorded on the triggering LKI snapshot onto each selected permanent.
    TransferFromTriggeringLki,
    /// Distribute `total` counters of `kind` among announced targets.
    Distribute { kind: CounterKind, total: u16 },
}

/// Combat and pairing mutations executed by [`ObjectMutation::Combat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectCombatOp {
    /// Remove each selected permanent from combat (CR 506.4).
    RemoveFromCombat,
    /// Reselect the defending player/planeswalker of the resolving attacking permanent.
    ReselectAttackDefender,
    /// Enlist (CR 702.154): optionally tap an untapped non-attacking creature to pump attacker.
    Enlist,
    /// Soulbond pairing (`entrant_only` distinguishes entrant vs existing trigger).
    SoulbondPair { entrant_only: bool },
}

/// Object state-marker mutations executed by [`ObjectMutation::StateMarker`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectStateMarkerOp {
    /// Grant flashback (equal to mana cost) until end of turn to each selected graveyard instant/sorcery.
    GrantFlashback,
    /// Mark the resolving Case permanent solved (CR 719.3).
    SolveCase,
    /// Mark the resolving permanent prepared.
    SetPrepared,
    /// Monstrosity N on the resolving permanent (CR 701.31).
    BecomeMonstrous { amount: ValueExpr },
    /// Unlock (or lock when `allow_lock` is true) a Room door on each selected permanent (CR 709.5).
    UnlockDoor {
        door: Option<SplitHalf>,
        allow_lock: bool,
    },
    /// Advance Class enchantment level to `u8` (CR 716.2a).
    AdvanceClassLevel(u8),
    /// Remember the targeted permanent in resolution cursor state for later instructions.
    RememberPermanentTarget,
}

/// Keyword zone-transition actions executed by [`ObjectMutation::KeywordZoneAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordZoneOp {
    /// Sacrifice the resolving evoked permanent if it was cast for its evoke cost.
    SacrificeEvoked,
    /// Sacrifice the resolving source of a state-triggered ability.
    SacrificeStateTriggerSource,
    /// Exile a defeated Siege battle before casting it transformed.
    ExileDefeatedSiege,
    /// Exile an unearthed permanent at end of turn or when leaving.
    ExileUnearthed(ObjectId),
    /// Schedule delayed artifact return at next end step.
    ScheduleArtifactReturn(ObjectId),
    /// Return an earthbent card from graveyard or exile to battlefield tapped.
    ReturnEarthbentCard,
}

/// Token creation, copy, and attachment actions executed by [`ObjectMutation::TokenAttach`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenAttachOp {
    /// Create an Aura token attached to selected host(s).
    CreateAuraAttached {
        definition: Box<crate::token::TokenDefinition>,
        up_to_one: bool,
    },
    /// Create a token and attach the resolving source (e.g. Living Weapon, For Mirrodin!, Job Select).
    CreateAndAttach {
        definition: Box<crate::token::TokenDefinition>,
    },
    /// Create token copies of selected object(s) using `TokenCopySpec`.
    CreateCopy(Box<TokenCopySpec>),
}

/// Orthogonal permanent/object state mutation executed by [`MutateObjectSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectMutation {
    /// Tap, untap, skip-next-untap, or exert each selected permanent.
    TapState(TapStateOp),
    /// Phase out, transform, or turn face up/down each selected permanent.
    Orientation(OrientationOp),
    /// Create a regeneration shield on each selected permanent (CR 701.15).
    Regenerate,
    /// Goad each selected creature under the first player resolved by `by` (CR 701.38).
    Goad { by: PlayerSelector, permanent: bool },
    /// Attach or unattach each selected attachment.
    Attach(AttachOp),
    /// Gain or exchange control of each selected permanent.
    Control(ControlOp),
    /// Add, remove, clear, double, move, transfer, or distribute counters.
    Counters(CounterOp),
    /// Combat removal, defender reselection, enlist, or soulbond pairing.
    Combat(ObjectCombatOp),
    /// Flashback grant, Case solve, prepared, monstrosity, Room door, Class level, or remembered target.
    StateMarker(ObjectStateMarkerOp),
    /// Earthbend N on a chosen controlled land.
    Earthbend { amount: ValueExpr },
    /// Explore (CR 701.40) on each selected permanent.
    Explore,
    /// Connive N (`Some(count)`) or finalize a connive discard (`None`) (CR 701.47).
    Connive { count: Option<ValueExpr> },
    /// Resolve a counter-based keyword trigger (modular, graft, bloodthirst, etc.).
    ResolveCounterKeyword(CounterKeyword),
    /// Resolve a growth keyword trigger (evolve, bolster-like growth, mentor, training, etc.).
    ResolveGrowth(GrowthKeyword),
    /// Resolve a transfer keyword trigger (soulshift, recover, etc.).
    ResolveTransfer(TransferKeyword),
    /// Resolve a combat trigger keyword (battle cry, exalted, mobilize, melee, etc.).
    ResolveKeywordTrigger(CombatTriggerKeyword),
    /// Resolve a power-related activated ability (crew, saddle, station, etc.).
    ResolvePowerAbility(crate::activated::PowerAbility),
    /// Execute a keyword zone-transition action (evoke, state-trigger sacrifice, siege, unearth, earthbend).
    KeywordZoneAction(KeywordZoneOp),
    /// Myriad trigger (CR 702.116): create attacking tapped token copies for each other opponent.
    Myriad,
    /// Unearth activation (CR 702.84): return source from graveyard with haste and exile delayed trigger.
    Unearth,
    /// Ninjutsu activation (CR 702.49): put source from hand onto battlefield tapped and attacking.
    Ninjutsu,
    /// Create attached Aura tokens, Living Weapon/Job Select tokens, or token copies.
    TokenAttach(TokenAttachOp),
}

impl ObjectMutation {
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Goad { by, .. } => by.uses_target(),
            Self::Attach(AttachOp::ToObject { host, ability }) => {
                ability.is_some() || host.uses_target()
            }
            Self::Attach(AttachOp::ToPlayer { player }) => player.uses_target(),
            Self::Attach(AttachOp::Unattach) => false,
            Self::Control(ControlOp::Gain { controller, .. }) => controller.uses_target(),
            Self::Control(ControlOp::Exchange { other }) => other.uses_target(),
            Self::Counters(CounterOp::Add { amount, .. } | CounterOp::Remove { amount, .. })
            | Self::StateMarker(ObjectStateMarkerOp::BecomeMonstrous { amount })
            | Self::Earthbend { amount } => amount.uses_target(),
            Self::Counters(CounterOp::Distribute { .. })
            | Self::StateMarker(ObjectStateMarkerOp::RememberPermanentTarget) => true,
            Self::Counters(CounterOp::Move { to, amount, .. }) => {
                to.uses_target() || amount.as_ref().is_some_and(ValueExpr::uses_target)
            }
            Self::Connive { count } => count.as_ref().is_some_and(ValueExpr::uses_target),
            Self::TokenAttach(TokenAttachOp::CreateCopy(copy)) => {
                copy.count.uses_target()
                    || matches!(
                        copy.recipients,
                        crate::token::TokenRecipients::Player(p)
                            if PlayerSelector::Resolving(p).uses_target()
                    )
                    || matches!(
                        copy.defender_scope,
                        crate::token::AttackingDefenderScope::Player(p)
                            | crate::token::AttackingDefenderScope::PlayerOrTheirPlaneswalker(p)
                            if PlayerSelector::Resolving(p).uses_target()
                    )
            }
            Self::TapState(_)
            | Self::Orientation(_)
            | Self::Regenerate
            | Self::Counters(
                CounterOp::Clear { .. }
                | CounterOp::Double { .. }
                | CounterOp::AddOneOfEach
                | CounterOp::TransferFromTriggeringLki,
            )
            | Self::Combat(_)
            | Self::StateMarker(
                ObjectStateMarkerOp::GrantFlashback
                | ObjectStateMarkerOp::SolveCase
                | ObjectStateMarkerOp::SetPrepared
                | ObjectStateMarkerOp::UnlockDoor { .. }
                | ObjectStateMarkerOp::AdvanceClassLevel(_),
            )
            | Self::Explore
            | Self::ResolveCounterKeyword(_)
            | Self::ResolveGrowth(_)
            | Self::ResolveTransfer(_)
            | Self::ResolveKeywordTrigger(_)
            | Self::ResolvePowerAbility(_)
            | Self::KeywordZoneAction(_)
            | Self::Myriad
            | Self::Unearth
            | Self::Ninjutsu
            | Self::TokenAttach(
                TokenAttachOp::CreateAuraAttached { .. } | TokenAttachOp::CreateAndAttach { .. },
            ) => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Goad { by, .. } => by.validate_vocabulary(vocabulary),
            Self::Attach(AttachOp::ToObject { host, .. }) => host.validate_vocabulary(vocabulary),
            Self::Attach(AttachOp::ToPlayer { player }) => player.validate_vocabulary(vocabulary),
            Self::Attach(AttachOp::Unattach) => Ok(()),
            Self::Control(ControlOp::Gain { controller, .. }) => {
                controller.validate_vocabulary(vocabulary)
            }
            Self::Control(ControlOp::Exchange { other }) => other.validate_vocabulary(vocabulary),
            Self::Counters(CounterOp::Add { amount, .. } | CounterOp::Remove { amount, .. })
            | Self::StateMarker(ObjectStateMarkerOp::BecomeMonstrous { amount })
            | Self::Earthbend { amount } => amount
                .validate_vocabulary(vocabulary)
                .map_err(|_| DefinitionError::InvalidInstruction),
            Self::Counters(CounterOp::Move { to, amount, .. }) => {
                to.validate_vocabulary(vocabulary)?;
                if let Some(expr) = amount {
                    expr.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                Ok(())
            }
            Self::Connive { count } => {
                if let Some(expr) = count {
                    expr.validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
                Ok(())
            }
            Self::TokenAttach(
                TokenAttachOp::CreateAuraAttached { definition, .. }
                | TokenAttachOp::CreateAndAttach { definition },
            ) => definition
                .validate(vocabulary)
                .map_err(|_| DefinitionError::TokenDefinition),
            Self::TokenAttach(TokenAttachOp::CreateCopy(copy)) => {
                copy.count
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                if copy
                    .extra_subtypes
                    .iter()
                    .any(|s| vocabulary.label(*s).is_none())
                {
                    return Err(DefinitionError::ForeignCreatureType);
                }
                for (kind, _) in &copy.extra_counters {
                    if !kind.is_permanent() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                }
                for ability in &copy.extra_activated {
                    if !ability.mana_cost.valid() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    ability
                        .body
                        .validate_in(vocabulary, crate::program::BodyContext::ACTIVATED)?;
                }
                for trigger in &copy.extra_triggered {
                    trigger.validate_body(vocabulary)?;
                }
                for stat in &copy.extra_static {
                    stat.validate(vocabulary)
                        .map_err(|_| DefinitionError::ContinuousEffect)?;
                }
                for mana in &copy.extra_mana {
                    if !mana.output.valid(vocabulary) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                }
                Ok(())
            }
            Self::TapState(_)
            | Self::Orientation(_)
            | Self::Regenerate
            | Self::Counters(
                CounterOp::Clear { .. }
                | CounterOp::Double { .. }
                | CounterOp::AddOneOfEach
                | CounterOp::TransferFromTriggeringLki
                | CounterOp::Distribute { .. },
            )
            | Self::Combat(_)
            | Self::StateMarker(
                ObjectStateMarkerOp::GrantFlashback
                | ObjectStateMarkerOp::SolveCase
                | ObjectStateMarkerOp::SetPrepared
                | ObjectStateMarkerOp::UnlockDoor { .. }
                | ObjectStateMarkerOp::AdvanceClassLevel(_)
                | ObjectStateMarkerOp::RememberPermanentTarget,
            )
            | Self::Explore
            | Self::ResolveCounterKeyword(_)
            | Self::ResolveGrowth(_)
            | Self::ResolveTransfer(_)
            | Self::ResolveKeywordTrigger(_)
            | Self::ResolvePowerAbility(_)
            | Self::KeywordZoneAction(_)
            | Self::Myriad
            | Self::Unearth
            | Self::Ninjutsu => Ok(()),
        }
    }
}

/// Complete universal object-mutation specification (`Source -> Select -> Mutate -> Bind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutateObjectSpec {
    pub source: ObjectQuery,
    pub selection: ZoneSelection,
    pub op: ObjectMutation,
    pub bind_mutated: Option<BindingId>,
    pub bind_succeeded: Option<BindingId>,
}

impl MutateObjectSpec {
    /// Construct a `MutateObjectSpec` applying `op` to every object returned by `source`.
    #[must_use]
    pub fn all(source: ObjectQuery, op: ObjectMutation) -> Self {
        Self {
            source,
            selection: ZoneSelection::All,
            op,
            bind_mutated: None,
            bind_succeeded: None,
        }
    }

    /// Construct a `MutateObjectSpec` with an explicit [`ZoneSelection`].
    #[must_use]
    pub fn with_selection(
        source: ObjectQuery,
        selection: ZoneSelection,
        op: ObjectMutation,
    ) -> Self {
        Self {
            source,
            selection,
            op,
            bind_mutated: None,
            bind_succeeded: None,
        }
    }

    #[must_use]
    pub fn is_pass_c_op(&self) -> bool {
        matches!(
            self.op,
            ObjectMutation::StateMarker(
                ObjectStateMarkerOp::BecomeMonstrous { .. }
                    | ObjectStateMarkerOp::UnlockDoor { .. }
            ) | ObjectMutation::Combat(ObjectCombatOp::Enlist)
        )
    }

    /// Goad each object returned by `source` under `by` until `by`'s next turn (CR 701.38).
    #[must_use]
    pub fn goad(source: ObjectQuery, by: PlayerSelector) -> Self {
        Self::all(
            source,
            ObjectMutation::Goad {
                by,
                permanent: false,
            },
        )
    }

    /// Goad each object returned by `source` under `by` for the rest of the game (CR 701.38).
    #[must_use]
    pub fn goad_permanently(source: ObjectQuery, by: PlayerSelector) -> Self {
        Self::all(
            source,
            ObjectMutation::Goad {
                by,
                permanent: true,
            },
        )
    }

    #[must_use]
    pub fn solve_case() -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::StateMarker(ObjectStateMarkerOp::SolveCase),
        )
    }

    #[must_use]
    pub fn set_prepared() -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::StateMarker(ObjectStateMarkerOp::SetPrepared),
        )
    }

    #[must_use]
    pub fn monstrosity(amount: ValueExpr) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::StateMarker(ObjectStateMarkerOp::BecomeMonstrous { amount }),
        )
    }

    #[must_use]
    pub fn unlock_door(source: ObjectQuery, door: Option<SplitHalf>, allow_lock: bool) -> Self {
        Self::all(
            source,
            ObjectMutation::StateMarker(ObjectStateMarkerOp::UnlockDoor { door, allow_lock }),
        )
    }

    #[must_use]
    pub fn earthbend(amount: ValueExpr) -> Self {
        Self::all(ObjectQuery::Targets, ObjectMutation::Earthbend { amount })
    }

    #[must_use]
    pub fn distribute_counters(kind: CounterKind, total: u16) -> Self {
        Self::all(
            ObjectQuery::Targets,
            ObjectMutation::Counters(CounterOp::Distribute { kind, total }),
        )
    }

    #[must_use]
    pub fn enlist() -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::Combat(ObjectCombatOp::Enlist),
        )
    }

    #[must_use]
    pub fn advance_class_level(level: u8) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::StateMarker(ObjectStateMarkerOp::AdvanceClassLevel(level)),
        )
    }

    #[must_use]
    pub fn reselect_attack_defender() -> Self {
        Self::all(
            ObjectQuery::Targets,
            ObjectMutation::Combat(ObjectCombatOp::ReselectAttackDefender),
        )
    }

    #[must_use]
    pub fn soulbond_pair(entrant_only: bool) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::Combat(ObjectCombatOp::SoulbondPair { entrant_only }),
        )
    }

    #[must_use]
    pub fn explore(source: ObjectQuery) -> Self {
        Self::all(source, ObjectMutation::Explore)
    }

    #[must_use]
    pub fn is_progress_or_choice_op(&self) -> bool {
        self.is_pass_c_op()
            || matches!(
                self.op,
                ObjectMutation::Connive { .. }
                    | ObjectMutation::ResolveCounterKeyword(_)
                    | ObjectMutation::ResolveGrowth(_)
                    | ObjectMutation::ResolveTransfer(_)
                    | ObjectMutation::ResolveKeywordTrigger(_)
                    | ObjectMutation::ResolvePowerAbility(_)
                    | ObjectMutation::KeywordZoneAction(_)
                    | ObjectMutation::Myriad
                    | ObjectMutation::Unearth
                    | ObjectMutation::Ninjutsu
                    | ObjectMutation::TokenAttach(_)
                    | ObjectMutation::StateMarker(ObjectStateMarkerOp::RememberPermanentTarget)
            )
    }

    #[must_use]
    pub fn connive(source: ObjectQuery, count: ValueExpr) -> Self {
        Self::all(source, ObjectMutation::Connive { count: Some(count) })
    }

    #[must_use]
    pub fn connive_finalize(source: ObjectQuery) -> Self {
        Self::all(source, ObjectMutation::Connive { count: None })
    }

    #[must_use]
    pub fn resolve_counter_keyword(kw: CounterKeyword) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::ResolveCounterKeyword(kw),
        )
    }

    #[must_use]
    pub fn resolve_growth(kw: GrowthKeyword) -> Self {
        Self::all(ObjectQuery::Source, ObjectMutation::ResolveGrowth(kw))
    }

    #[must_use]
    pub fn resolve_transfer(kw: TransferKeyword) -> Self {
        Self::all(ObjectQuery::Source, ObjectMutation::ResolveTransfer(kw))
    }

    #[must_use]
    pub fn resolve_keyword_trigger(kw: CombatTriggerKeyword) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::ResolveKeywordTrigger(kw),
        )
    }

    #[must_use]
    pub fn resolve_power_ability(ability: crate::activated::PowerAbility) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::ResolvePowerAbility(ability),
        )
    }

    #[must_use]
    pub fn sacrifice_evoked() -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::KeywordZoneAction(KeywordZoneOp::SacrificeEvoked),
        )
    }

    #[must_use]
    pub fn sacrifice_state_trigger_source() -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::KeywordZoneAction(KeywordZoneOp::SacrificeStateTriggerSource),
        )
    }

    #[must_use]
    pub fn myriad() -> Self {
        Self::all(ObjectQuery::Source, ObjectMutation::Myriad)
    }

    #[must_use]
    pub fn unearth() -> Self {
        Self::all(ObjectQuery::Source, ObjectMutation::Unearth)
    }

    #[must_use]
    pub fn ninjutsu() -> Self {
        Self::all(ObjectQuery::Source, ObjectMutation::Ninjutsu)
    }

    #[must_use]
    pub fn exile_defeated_siege() -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::KeywordZoneAction(KeywordZoneOp::ExileDefeatedSiege),
        )
    }

    #[must_use]
    pub fn exile_unearthed(object: ObjectId) -> Self {
        Self::all(
            ObjectQuery::Fixed(vec![object]),
            ObjectMutation::KeywordZoneAction(KeywordZoneOp::ExileUnearthed(object)),
        )
    }

    #[must_use]
    pub fn schedule_artifact_return(object: ObjectId) -> Self {
        Self::all(
            ObjectQuery::Fixed(vec![object]),
            ObjectMutation::KeywordZoneAction(KeywordZoneOp::ScheduleArtifactReturn(object)),
        )
    }

    #[must_use]
    pub fn return_earthbent_card() -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::KeywordZoneAction(KeywordZoneOp::ReturnEarthbentCard),
        )
    }

    #[must_use]
    pub fn create_aura_token_attached(
        host: ObjectQuery,
        definition: Box<crate::token::TokenDefinition>,
        up_to_one: bool,
    ) -> Self {
        Self::all(
            host,
            ObjectMutation::TokenAttach(TokenAttachOp::CreateAuraAttached {
                definition,
                up_to_one,
            }),
        )
    }

    #[must_use]
    pub fn create_token_and_attach(definition: Box<crate::token::TokenDefinition>) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::TokenAttach(TokenAttachOp::CreateAndAttach { definition }),
        )
    }

    #[must_use]
    pub fn create_token_copy(source: ObjectQuery, copy: Box<TokenCopySpec>) -> Self {
        Self::all(
            source,
            ObjectMutation::TokenAttach(TokenAttachOp::CreateCopy(copy)),
        )
    }

    #[must_use]
    pub fn remember_permanent_target() -> Self {
        Self::all(
            ObjectQuery::Targets,
            ObjectMutation::StateMarker(ObjectStateMarkerOp::RememberPermanentTarget),
        )
    }

    /// Attach the resolving source using an Equip, Fortify, or Reconfigure ability.
    #[must_use]
    pub fn attach_ability(ability: crate::attachment::AttachAbility) -> Self {
        Self::all(
            ObjectQuery::Source,
            ObjectMutation::Attach(AttachOp::ToObject {
                host: Box::new(ObjectQuery::Targets),
                ability: Some(ability),
            }),
        )
    }

    /// Exchange control of two battlefield permanents (CR 701.10a).
    #[must_use]
    pub fn exchange_control(first: ObjectQuery, second: ObjectQuery) -> Self {
        Self::all(
            first,
            ObjectMutation::Control(ControlOp::Exchange {
                other: Box::new(second),
            }),
        )
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.source.uses_target() || self.selection.uses_target() || self.op.uses_target()
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.source.validate_vocabulary(vocabulary)?;
        self.selection.validate_vocabulary(vocabulary)?;
        self.op.validate_vocabulary(vocabulary)
    }
}
