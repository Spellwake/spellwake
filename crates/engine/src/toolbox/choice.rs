//! Universal `PromptChoice` engine, choice history, and `ChoiceMade` event metadata.
//!
//! Replaces bespoke `*ChoiceFrame` structs with one generic interactive choice
//! packet supporting objects, players, options, numbers, colors, card types,
//! subtypes, card names, and N-pile partitions. Every resolved choice records
//! history and emits a generic `ChoiceMade` occurrence.

use crate::{
    effects::EffectError,
    land::LandType,
    mana::ManaType,
    object_identity::ObjectId,
    priority::SeatId,
    program::{CardType, ColorMask, DefinitionError, PlaneswalkerSubtype, ValueExpr},
    stack_window::StackEntryId,
    toolbox::{
        env::BindingId,
        query::{ObjectQuery, PlayerSelector, QueryZone},
        tags::TagKey,
        zone_op::SelectionConstraint,
    },
};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary},
};
use std::collections::BTreeSet;

/// Semantic category of a [`PromptChoiceSpec`], carried into choice history and
/// `RulesOccurrence::ChoiceMade` so triggers like Doctor Who villainous choices,
/// Star Trek dilemmas, votes, and modal watchers reuse one event pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoiceCategory {
    Generic,
    Modal,
    VillainousChoice,
    Dilemma,
    Vote,
    PileSplit,
    Search,
    Tribute,
    Clash,
    Custom(u16),
}

/// Which subtype family is offered by [`PromptDomain::Subtype`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtypeChoiceKind {
    Creature,
    BasicLand,
    Planeswalker,
}

/// Program-time domain specification for `Instruction::PromptChoice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptDomain {
    /// Choose `min..=max` objects from `query`.
    Objects {
        query: ObjectQuery,
        min: Box<ValueExpr>,
        max: Box<ValueExpr>,
        constraint: Option<SelectionConstraint>,
        max_total_power: Option<Box<ValueExpr>>,
        hidden: bool,
    },
    /// Choose `min..=max` players from `selector`.
    Players {
        selector: PlayerSelector,
        min: u8,
        max: u8,
        random: bool,
    },
    /// Choose `min..=max` labeled options.
    Options {
        labels: Vec<String>,
        min: u8,
        max: u8,
        allow_repeat: bool,
    },
    /// Choose an integer in `min..=max`.
    Number {
        min: Box<ValueExpr>,
        max: Box<ValueExpr>,
    },
    /// Choose one color from `allowed` (or all five WUBRG colors when empty).
    Color { allowed: ColorMask },
    /// Choose one card type from `allowed` (or the 7 standard types when empty).
    CardType { allowed: Vec<CardType> },
    /// Choose a subtype from a typed family.
    Subtype {
        kind: SubtypeChoiceKind,
        excluding: BTreeSet<CreatureTypeId>,
        legacy_slot: Option<crate::program::CreatureTypeSlot>,
    },
    /// Choose a card name (optionally nonland only).
    CardName { nonland_only: bool },
    /// Partition all objects from `query` into `pile_count` disjoint piles (e.g. Fact or Fiction,
    /// Raging River, Space Beleren).
    Partition {
        query: ObjectQuery,
        pile_count: u8,
        min_per_pile: u8,
    },
    /// Offer a mana payment with `fixed` plus dynamically evaluated `extra_generic`,
    /// jumping to `paid_destination` if paid or `declined_destination` if declined.
    ManaPayment {
        fixed: crate::mana::ManaCost,
        extra_generic: Box<ValueExpr>,
        paid_destination: usize,
        declined_destination: usize,
    },
}

impl PromptDomain {
    /// Choose one of the five MTG colors.
    #[must_use]
    pub const fn color() -> Self {
        Self::Color {
            allowed: ColorMask::ALL,
        }
    }

    /// Choose a single player (or opponent when `opponents_only` is true).
    #[must_use]
    pub fn player(opponents_only: bool) -> Self {
        Self::Players {
            selector: PlayerSelector::Group(if opponents_only {
                crate::program::AffectedPlayers::EachOpponent
            } else {
                crate::program::AffectedPlayers::EachPlayer
            }),
            min: 1,
            max: 1,
            random: false,
        }
    }

    /// Choose a single player (or opponent when `opponents_only` is true) at random.
    #[must_use]
    pub fn random_player(opponents_only: bool) -> Self {
        Self::Players {
            selector: PlayerSelector::Group(if opponents_only {
                crate::program::AffectedPlayers::EachOpponent
            } else {
                crate::program::AffectedPlayers::EachPlayer
            }),
            min: 1,
            max: 1,
            random: true,
        }
    }

    /// Choose one card type from `allowed` (or the 7 standard types when empty).
    #[must_use]
    pub const fn card_type(allowed: Vec<CardType>) -> Self {
        Self::CardType { allowed }
    }

    /// Choose one of the five basic land types.
    #[must_use]
    pub fn basic_land_type() -> Self {
        Self::Subtype {
            kind: SubtypeChoiceKind::BasicLand,
            excluding: BTreeSet::new(),
            legacy_slot: None,
        }
    }

    /// Choose an integer in `min..=max`.
    #[must_use]
    pub fn number_range(min: u16, max: u16) -> Self {
        Self::Number {
            min: Box::new(ValueExpr::Literal(RulesInteger::from(i64::from(min)))),
            max: Box::new(ValueExpr::Literal(RulesInteger::from(i64::from(max)))),
        }
    }

    /// Choose one labeled option from `labels`.
    #[must_use]
    pub fn labeled_option(labels: Vec<String>) -> Self {
        Self::Options {
            labels,
            min: 1,
            max: 1,
            allow_repeat: false,
        }
    }

    /// Choose a card name (optionally restricted to nonland cards).
    #[must_use]
    pub const fn card_name(nonland_only: bool) -> Self {
        Self::CardName { nonland_only }
    }

    /// Whether any part of this domain depends on an announced target.
    #[must_use]
    pub fn uses_target(&self) -> bool {
        match self {
            Self::Objects {
                query,
                min,
                max,
                max_total_power,
                ..
            } => {
                query.uses_target()
                    || min.uses_target()
                    || max.uses_target()
                    || max_total_power
                        .as_deref()
                        .is_some_and(ValueExpr::uses_target)
            }
            Self::Players { selector, .. } => selector.uses_target(),
            Self::Number { min, max } => min.uses_target() || max.uses_target(),
            Self::Partition { query, .. } => query.uses_target(),
            Self::ManaPayment { extra_generic, .. } => extra_generic.uses_target(),
            Self::Options { .. }
            | Self::Color { .. }
            | Self::CardType { .. }
            | Self::Subtype { .. }
            | Self::CardName { .. } => false,
        }
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        match self {
            Self::Objects {
                query,
                min,
                max,
                max_total_power,
                ..
            } => {
                query.validate_vocabulary(vocabulary)?;
                min.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                max.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                if let Some(limit) = max_total_power {
                    limit
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                }
            }
            Self::Players {
                selector, min, max, ..
            } => {
                if min > max {
                    return Err(DefinitionError::InvalidInstruction);
                }
                selector.validate_vocabulary(vocabulary)?;
            }
            Self::Options {
                labels, min, max, ..
            } => {
                if labels.is_empty() || labels.len() > usize::from(u8::MAX) || min > max {
                    return Err(DefinitionError::InvalidInstruction);
                }
            }
            Self::Number { min, max } => {
                min.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
                max.validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
            }
            Self::Partition {
                query, pile_count, ..
            } => {
                if *pile_count < 2 {
                    return Err(DefinitionError::InvalidInstruction);
                }
                query.validate_vocabulary(vocabulary)?;
            }
            Self::ManaPayment { extra_generic, .. } => {
                extra_generic
                    .validate_vocabulary(vocabulary)
                    .map_err(|_| DefinitionError::InvalidInstruction)?;
            }
            Self::Subtype {
                kind, excluding, ..
            } => {
                if excluding.iter().any(|id| vocabulary.label(*id).is_none()) {
                    return Err(DefinitionError::ForeignCreatureType);
                }
                if *kind == SubtypeChoiceKind::Creature && excluding.len() == vocabulary.len() {
                    return Err(DefinitionError::EmptyCreatureTypeDomain);
                }
            }
            Self::Color { .. } | Self::CardType { .. } | Self::CardName { .. } => {}
        }
        Ok(())
    }
}

/// Complete program specification for `Instruction::PromptChoice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChoiceSpec {
    pub chooser: PlayerSelector,
    pub domain: PromptDomain,
    pub category: ChoiceCategory,
    /// Primary binding slot receiving the chosen value in [`crate::toolbox::ExecEnv`].
    pub bind_to: BindingId,
    /// For `Objects` domains, optional slot receiving unchosen candidates.
    pub bind_unchosen: Option<BindingId>,
    /// For `Partition` domains, optional tag key tagging every object with its assigned pile index.
    pub tag_partition: Option<TagKey>,
    /// When true, skip prompting if there is at most one legal answer (e.g. 0 candidates with min 0).
    pub auto_resolve_trivial: bool,
    /// Optional per-option executable branches spliced at resolution time for `PromptDomain::Options`.
    pub option_branches: Vec<crate::program::ExecutableBody>,
}

impl PromptChoiceSpec {
    /// Binding slot populated with unchosen objects by [`Self::choose_objects_instructions`].
    #[must_use]
    pub const fn unchosen_binding() -> BindingId {
        BindingId(1)
    }

    /// Construct a creature-type choice spec bound to a legacy [`crate::program::CreatureTypeSlot`].
    #[must_use]
    pub fn choose_creature_type(
        into: crate::program::CreatureTypeSlot,
        excluding: BTreeSet<CreatureTypeId>,
    ) -> Self {
        Self {
            chooser: PlayerSelector::Resolving(crate::program::ResolvingPlayer::Controller),
            domain: PromptDomain::Subtype {
                kind: SubtypeChoiceKind::Creature,
                excluding,
                legacy_slot: Some(into),
            },
            category: ChoiceCategory::Generic,
            bind_to: BindingId(9900u16.saturating_add(into.ordinal())),
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: false,
            option_branches: Vec::new(),
        }
    }

    /// Construct a resolution-time non-object value choice prompt for `chooser`.
    #[must_use]
    pub fn choose_value(chooser: crate::program::ResolvingPlayer, domain: PromptDomain) -> Self {
        Self {
            chooser: PlayerSelector::Resolving(chooser),
            domain,
            category: ChoiceCategory::Generic,
            bind_to: BindingId(0),
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: true,
            option_branches: Vec::new(),
        }
    }

    /// Construct a resolution-time modal branch choice prompt for `chooser`.
    #[must_use]
    pub fn choose_option(
        chooser: crate::program::ResolvingPlayer,
        options: Vec<(String, crate::program::ExecutableBody)>,
    ) -> Self {
        let (labels, option_branches): (Vec<_>, Vec<_>) = options.into_iter().unzip();
        Self {
            chooser: PlayerSelector::Resolving(chooser),
            domain: PromptDomain::Options {
                labels,
                min: 1,
                max: 1,
                allow_repeat: false,
            },
            category: ChoiceCategory::Modal,
            bind_to: BindingId(0),
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: false,
            option_branches,
        }
    }

    /// Construct a dynamic mana payment prompt offering `payer` to pay `fixed` plus
    /// `extra_generic`, jumping to `paid_destination` if paid or `declined_destination` if declined.
    #[must_use]
    pub fn offer_dynamic_mana_payment(
        payer: crate::program::ResolvingPlayer,
        fixed: crate::mana::ManaCost,
        extra_generic: ValueExpr,
        paid_destination: usize,
        declined_destination: usize,
    ) -> Self {
        Self {
            chooser: PlayerSelector::Resolving(payer),
            domain: PromptDomain::ManaPayment {
                fixed,
                extra_generic: Box::new(extra_generic),
                paid_destination,
                declined_destination,
            },
            category: ChoiceCategory::Generic,
            bind_to: BindingId(0),
            bind_unchosen: None,
            tag_partition: None,
            auto_resolve_trivial: false,
            option_branches: Vec::new(),
        }
    }

    /// Inspect this spec as a dynamic mana payment prompt, if it uses `PromptDomain::ManaPayment`
    /// with a `PlayerSelector::Resolving` chooser.
    #[must_use]
    pub fn as_mana_payment(
        &self,
    ) -> Option<(
        crate::program::ResolvingPlayer,
        &crate::mana::ManaCost,
        &ValueExpr,
        usize,
        usize,
    )> {
        let PlayerSelector::Resolving(payer) = self.chooser else {
            return None;
        };
        let PromptDomain::ManaPayment {
            fixed,
            extra_generic,
            paid_destination,
            declined_destination,
        } = &self.domain
        else {
            return None;
        };
        Some((
            payer,
            fixed,
            extra_generic.as_ref(),
            *paid_destination,
            *declined_destination,
        ))
    }

    /// Mutably borrow the fixed mana cost and jump destinations of a `PromptDomain::ManaPayment` spec.
    #[must_use]
    pub fn as_mana_payment_mut(
        &mut self,
    ) -> Option<(&mut crate::mana::ManaCost, &mut usize, &mut usize)> {
        let PromptDomain::ManaPayment {
            fixed,
            paid_destination,
            declined_destination,
            ..
        } = &mut self.domain
        else {
            return None;
        };
        Some((fixed, paid_destination, declined_destination))
    }

    /// Translate a legacy [`crate::program::SourceZoneFilter`] and actor into an [`ObjectQuery`].
    #[must_use]
    pub fn objects_query_for_source_zone(
        source_zone: crate::program::SourceZoneFilter,
        actor: PlayerSelector,
        filter: crate::program::ObjectFilter,
    ) -> ObjectQuery {
        use crate::program::{AffectedPlayers, ResolvingPlayer, SourceZoneFilter};
        match source_zone {
            SourceZoneFilter::Hand => ObjectQuery::zone(QueryZone::Hand(Box::new(actor)), filter),
            SourceZoneFilter::Graveyard => {
                ObjectQuery::zone(QueryZone::Graveyard(Box::new(actor)), filter)
            }
            SourceZoneFilter::HandOrGraveyard => ObjectQuery::Union(vec![
                ObjectQuery::zone(QueryZone::Hand(Box::new(actor.clone())), filter),
                ObjectQuery::zone(QueryZone::Graveyard(Box::new(actor)), filter),
            ]),
            SourceZoneFilter::Battlefield => {
                ObjectQuery::zone(QueryZone::BattlefieldControlledBy(Box::new(actor)), filter)
            }
            SourceZoneFilter::AnyBattlefield => ObjectQuery::zone(QueryZone::Battlefield, filter),
            SourceZoneFilter::Exile => ObjectQuery::zone(QueryZone::Exile, filter),
            SourceZoneFilter::OpponentGraveyard => ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Group(
                    AffectedPlayers::EachOpponent,
                ))),
                filter,
            ),
            SourceZoneFilter::AnyGraveyard => ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Group(AffectedPlayers::EachPlayer))),
                filter,
            ),
            SourceZoneFilter::YourLibrary => {
                ObjectQuery::zone(QueryZone::Library(Box::new(actor)), filter)
            }
            SourceZoneFilter::OpponentExile => ObjectQuery::zone(
                QueryZone::ExileOwnedBy(Box::new(PlayerSelector::Group(
                    AffectedPlayers::EachOpponent,
                ))),
                filter,
            ),
            SourceZoneFilter::LinkedExile => {
                ObjectQuery::filtered(ObjectQuery::LinkedExile { group: None }, filter)
            }
            SourceZoneFilter::LastMoved => ObjectQuery::filtered(ObjectQuery::LastMoved, filter),
            SourceZoneFilter::TargetPlayerHand => ObjectQuery::zone(
                QueryZone::Hand(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                filter,
            ),
            SourceZoneFilter::TargetPlayerGraveyard => ObjectQuery::zone(
                QueryZone::Graveyard(Box::new(PlayerSelector::Resolving(
                    ResolvingPlayer::TargetPlayer,
                ))),
                filter,
            ),
            SourceZoneFilter::OpponentHand => ObjectQuery::zone(
                QueryZone::Hand(Box::new(PlayerSelector::Group(
                    AffectedPlayers::EachOpponent,
                ))),
                filter,
            ),
            SourceZoneFilter::CommandZone => ObjectQuery::zone(QueryZone::Command, filter),
        }
    }

    /// Construct a resolution-time object selection prompt over `query`.
    #[must_use]
    pub fn choose_objects(
        chooser: PlayerSelector,
        query: ObjectQuery,
        min: ValueExpr,
        max: ValueExpr,
        distinct_powers: bool,
        max_total_power: Option<ValueExpr>,
        bind_unchosen: Option<BindingId>,
    ) -> Self {
        Self {
            chooser,
            domain: PromptDomain::Objects {
                query,
                min: Box::new(min),
                max: Box::new(max),
                constraint: distinct_powers.then_some(SelectionConstraint::DistinctPowers),
                max_total_power: max_total_power.map(Box::new),
                hidden: false,
            },
            category: ChoiceCategory::Generic,
            bind_to: BindingId(0),
            bind_unchosen,
            tag_partition: None,
            auto_resolve_trivial: true,
            option_branches: Vec::new(),
        }
    }

    /// Lower a single- or multi-player object selection (with optional unchosen follow-up bound in
    /// [`Self::unchosen_binding`]) into toolbox instructions.
    // All 8 parameters correspond directly to the orthogonal object-selection specification fields.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn choose_objects_instructions(
        players: crate::program::AffectedPlayers,
        source_zone: crate::program::SourceZoneFilter,
        filter: crate::program::ObjectFilter,
        min: ValueExpr,
        max: ValueExpr,
        distinct_powers: bool,
        max_total_power: Option<ValueExpr>,
        unchosen_followup: Option<crate::program::Instruction>,
    ) -> Vec<crate::program::Instruction> {
        use crate::program::{AffectedPlayers, BodyTarget, ExecutableBody, Instruction};
        let bind_unchosen = unchosen_followup.as_ref().map(|_| Self::unchosen_binding());
        if let AffectedPlayers::One(rp) = players {
            let actor = PlayerSelector::Resolving(rp);
            let query = Self::objects_query_for_source_zone(source_zone, actor.clone(), filter);
            let prompt = Instruction::PromptChoice(Box::new(Self::choose_objects(
                actor,
                query,
                min,
                max,
                distinct_powers,
                max_total_power,
                bind_unchosen,
            )));
            let mut ops = vec![prompt];
            if let Some(followup) = unchosen_followup {
                ops.push(followup);
            }
            ops
        } else {
            let actor = PlayerSelector::Bound(BindingId(2));
            let query = Self::objects_query_for_source_zone(source_zone, actor.clone(), filter);
            let prompt = Instruction::PromptChoice(Box::new(Self::choose_objects(
                actor,
                query,
                min,
                max,
                distinct_powers,
                max_total_power,
                bind_unchosen,
            )));
            let mut body_ops = vec![prompt];
            if let Some(followup) = unchosen_followup {
                body_ops.push(followup);
            }
            vec![Instruction::ForEachPlayerQuery {
                selector: Box::new(PlayerSelector::Group(players)),
                bind_player: BindingId(2),
                body: Box::new(ExecutableBody::new(BodyTarget::None, body_ops)),
            }]
        }
    }

    #[must_use]
    pub fn uses_target(&self) -> bool {
        self.chooser.uses_target()
            || self.domain.uses_target()
            || self.option_branches.iter().any(|branch| {
                branch
                    .instructions
                    .iter()
                    .any(crate::program::Instruction::uses_mode_target)
            })
    }

    pub(crate) fn validate_vocabulary(
        &self,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<(), DefinitionError> {
        self.chooser.validate_vocabulary(vocabulary)?;
        self.domain.validate_vocabulary(vocabulary)?;
        if !self.option_branches.is_empty() {
            let matches_labels = matches!(
                &self.domain,
                PromptDomain::Options { labels, .. } if labels.len() == self.option_branches.len()
            );
            if !matches_labels
                || self.option_branches.len() < 2
                || self.option_branches.len() > usize::from(u8::MAX)
            {
                return Err(DefinitionError::InvalidInstruction);
            }
        }
        Ok(())
    }
}

/// Executor-frozen legal domain issued to the wire for a single [`PromptChoiceFrame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPromptDomain {
    Objects {
        candidates: Vec<ObjectId>,
        min: usize,
        max: usize,
        hidden: bool,
    },
    Players {
        candidates: Vec<SeatId>,
        min: u8,
        max: u8,
    },
    Options {
        labels: Vec<String>,
        min: u8,
        max: u8,
        allow_repeat: bool,
    },
    Number {
        min: RulesInteger,
        max: RulesInteger,
    },
    Color {
        allowed: BTreeSet<ManaType>,
    },
    CardType {
        allowed: Vec<CardType>,
    },
    CreatureType {
        allowed: Vec<CreatureTypeId>,
    },
    BasicLandType {
        allowed: Vec<LandType>,
    },
    PlaneswalkerType {
        allowed: Vec<PlaneswalkerSubtype>,
    },
    CardName {
        nonland_only: bool,
    },
    Partition {
        candidates: Vec<ObjectId>,
        pile_count: u8,
        min_per_pile: u8,
    },
}

/// Single generic immutable choice frame emitted by the universal toolbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptChoiceFrame {
    pub revision: u64,
    pub entry: StackEntryId,
    pub instruction: usize,
    pub chooser: SeatId,
    pub category: ChoiceCategory,
    pub domain: ResolvedPromptDomain,
}

impl PromptChoiceFrame {
    /// Authorized seat that must answer this frame.
    #[must_use]
    pub const fn chooser(&self) -> SeatId {
        self.chooser
    }
}

/// Single generic player reply paired with [`PromptChoiceFrame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptChoiceReply {
    Objects(Vec<ObjectId>),
    Players(Vec<SeatId>),
    Options(Vec<u8>),
    Number(RulesInteger),
    Color(ManaType),
    CardType(CardType),
    CreatureType(CreatureTypeId),
    BasicLandType(LandType),
    PlaneswalkerType(PlaneswalkerSubtype),
    CardName(String),
    Partition(Vec<Vec<ObjectId>>),
}

/// Immutable history record appended whenever a [`PromptChoiceSpec`] resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRecord {
    pub turn: u64,
    pub chooser: SeatId,
    pub source: Option<ObjectId>,
    pub category: ChoiceCategory,
    pub option_indices: Vec<u8>,
    pub chosen_count: usize,
}

/// Atomically validate `reply` against `frame.domain` before mutating any engine state.
///
/// # Errors
/// Returns [`EffectError::WrongDecisionKind`] when the reply variant does not match the domain,
/// or [`EffectError::InvalidSelectionCount`] / [`EffectError::DuplicateSelection`] / [`EffectError::SelectionOutsideDomain`] when counts, uniqueness, membership, or partition
/// completeness invariants are violated.
pub fn validate_prompt_reply(
    frame: &PromptChoiceFrame,
    reply: &PromptChoiceReply,
) -> Result<(), EffectError> {
    match (&frame.domain, reply) {
        (
            ResolvedPromptDomain::Objects {
                candidates,
                min,
                max,
                ..
            },
            PromptChoiceReply::Objects(chosen),
        ) => {
            if chosen.len() < *min || chosen.len() > *max {
                return Err(EffectError::InvalidSelectionCount);
            }
            let allowed: BTreeSet<ObjectId> = candidates.iter().copied().collect();
            let mut seen = BTreeSet::new();
            for id in chosen {
                if !allowed.contains(id) {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                if !seen.insert(*id) {
                    return Err(EffectError::DuplicateSelection);
                }
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::Players {
                candidates,
                min,
                max,
            },
            PromptChoiceReply::Players(chosen),
        ) => {
            if chosen.len() < usize::from(*min) || chosen.len() > usize::from(*max) {
                return Err(EffectError::InvalidSelectionCount);
            }
            let allowed: BTreeSet<SeatId> = candidates.iter().copied().collect();
            let mut seen = BTreeSet::new();
            for seat in chosen {
                if !allowed.contains(seat) {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                if !seen.insert(*seat) {
                    return Err(EffectError::DuplicateSelection);
                }
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::Options {
                labels,
                min,
                max,
                allow_repeat,
            },
            PromptChoiceReply::Options(chosen),
        ) => {
            if chosen.len() < usize::from(*min) || chosen.len() > usize::from(*max) {
                return Err(EffectError::InvalidSelectionCount);
            }
            let mut seen = BTreeSet::new();
            for &idx in chosen {
                if usize::from(idx) >= labels.len() {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                if !*allow_repeat && !seen.insert(idx) {
                    return Err(EffectError::DuplicateSelection);
                }
            }
            Ok(())
        }
        (ResolvedPromptDomain::Number { min, max }, PromptChoiceReply::Number(val)) => {
            if val < min || val > max {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::Color { allowed }, PromptChoiceReply::Color(color)) => {
            if !allowed.contains(color) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::CardType { allowed }, PromptChoiceReply::CardType(ty)) => {
            if !allowed.contains(ty) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::CreatureType { allowed }, PromptChoiceReply::CreatureType(sub)) => {
            if !allowed.contains(sub) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::BasicLandType { allowed }, PromptChoiceReply::BasicLandType(lt)) => {
            if !allowed.contains(lt) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::PlaneswalkerType { allowed },
            PromptChoiceReply::PlaneswalkerType(pw),
        ) => {
            if !allowed.contains(pw) {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (ResolvedPromptDomain::CardName { .. }, PromptChoiceReply::CardName(name)) => {
            if name.trim().is_empty() {
                return Err(EffectError::SelectionOutsideDomain);
            }
            Ok(())
        }
        (
            ResolvedPromptDomain::Partition {
                candidates,
                pile_count,
                min_per_pile,
            },
            PromptChoiceReply::Partition(piles),
        ) => {
            if piles.len() != usize::from(*pile_count) {
                return Err(EffectError::InvalidSelectionCount);
            }
            let expected: BTreeSet<ObjectId> = candidates.iter().copied().collect();
            let mut assigned = BTreeSet::new();
            for pile in piles {
                if pile.len() < usize::from(*min_per_pile) {
                    return Err(EffectError::InvalidSelectionCount);
                }
                for id in pile {
                    if !expected.contains(id) {
                        return Err(EffectError::SelectionOutsideDomain);
                    }
                    if !assigned.insert(*id) {
                        return Err(EffectError::DuplicateSelection);
                    }
                }
            }
            if assigned.len() != expected.len() {
                return Err(EffectError::InvalidSelectionCount);
            }
            Ok(())
        }
        _ => Err(EffectError::WrongDecisionKind),
    }
}
