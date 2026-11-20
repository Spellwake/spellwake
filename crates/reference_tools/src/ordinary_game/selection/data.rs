//! Selected wire aliases use the maintained Case/Inputs field table unchanged.
use super::prepared::{PreparedManaType, SelectionInitialization};
use crate::ordinary_game::mandatory::data::{
    Ability, Case, EntryRef, Inputs, ObjectRef, Role, Target,
};
use serde::Deserialize;
use spellwake_contracts::RulesInteger;

/// Closed two-seat prepared input inventory. SD17 and SD20 have no fixture admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum SelectionCase {
    #[serde(rename = "SD01")]
    Sd01,
    #[serde(rename = "SD02")]
    Sd02,
    #[serde(rename = "SD03")]
    Sd03,
    #[serde(rename = "SD04")]
    Sd04,
    #[serde(rename = "SD05")]
    Sd05,
    #[serde(rename = "SD06")]
    Sd06,
    #[serde(rename = "SD07")]
    Sd07,
    #[serde(rename = "SD08")]
    Sd08,
    #[serde(rename = "SD09")]
    Sd09,
    #[serde(rename = "SD10")]
    Sd10,
    #[serde(rename = "SD11")]
    Sd11,
    #[serde(rename = "SD12")]
    Sd12,
    #[serde(rename = "SD13")]
    Sd13,
    #[serde(rename = "SD14")]
    Sd14,
    #[serde(rename = "SD15")]
    Sd15,
    #[serde(rename = "SD16")]
    Sd16,
    #[serde(rename = "SD18")]
    Sd18,
    #[serde(rename = "SD19")]
    Sd19,
    #[serde(rename = "sd04-real-history-companion")]
    Sd04RealHistoryCompanion,
    #[serde(rename = "sd14-real-history-companion")]
    Sd14RealHistoryCompanion,
}
impl SelectionCase {
    pub const ALL: [Self; 20] = [
        Self::Sd01,
        Self::Sd02,
        Self::Sd03,
        Self::Sd04,
        Self::Sd05,
        Self::Sd06,
        Self::Sd07,
        Self::Sd08,
        Self::Sd09,
        Self::Sd10,
        Self::Sd11,
        Self::Sd12,
        Self::Sd13,
        Self::Sd14,
        Self::Sd15,
        Self::Sd16,
        Self::Sd18,
        Self::Sd19,
        Self::Sd04RealHistoryCompanion,
        Self::Sd14RealHistoryCompanion,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sd01 => "SD01",
            Self::Sd02 => "SD02",
            Self::Sd03 => "SD03",
            Self::Sd04 => "SD04",
            Self::Sd05 => "SD05",
            Self::Sd06 => "SD06",
            Self::Sd07 => "SD07",
            Self::Sd08 => "SD08",
            Self::Sd09 => "SD09",
            Self::Sd10 => "SD10",
            Self::Sd11 => "SD11",
            Self::Sd12 => "SD12",
            Self::Sd13 => "SD13",
            Self::Sd14 => "SD14",
            Self::Sd15 => "SD15",
            Self::Sd16 => "SD16",
            Self::Sd18 => "SD18",
            Self::Sd19 => "SD19",
            Self::Sd04RealHistoryCompanion => "sd04-real-history-companion",
            Self::Sd14RealHistoryCompanion => "sd14-real-history-companion",
        }
    }
    /// Parse only the reviewed twenty exact wire identities.
    /// # Errors
    /// Rejects unknown names, including SD17 and SD20.
    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|case| case.as_str() == value)
            .ok_or_else(|| format!("unknown selection case {value}"))
    }
    pub(in crate::ordinary_game) const fn game_id(self) -> u128 {
        match self {
            Self::Sd01 => 72001,
            Self::Sd02 => 72002,
            Self::Sd03 => 72003,
            Self::Sd04 => 72004,
            Self::Sd05 => 72005,
            Self::Sd06 => 72006,
            Self::Sd07 => 72007,
            Self::Sd08 => 72008,
            Self::Sd09 => 72009,
            Self::Sd10 => 72010,
            Self::Sd11 => 72011,
            Self::Sd12 => 72012,
            Self::Sd13 => 72013,
            Self::Sd14 => 72014,
            Self::Sd15 => 72015,
            Self::Sd16 => 72016,
            Self::Sd18 => 72017,
            Self::Sd19 => 72018,
            Self::Sd04RealHistoryCompanion => 72019,
            Self::Sd14RealHistoryCompanion => 72020,
        }
    }
}

pub(in crate::ordinary_game) type SelectionInputCase =
    Case<SelectionCase, SelectionInitialization, SelectionDecisionPlan, PreparedManaType>;
pub(in crate::ordinary_game) type SelectionInputs =
    Inputs<SelectionCase, SelectionInitialization, SelectionDecisionPlan, PreparedManaType>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(in crate::ordinary_game) enum SelectionOperation {
    SacrificeCreature,
    DiscardCards,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum SelectionDecisionPlan {
    ResolutionSelect {
        id: String,
        entry: EntryRef,
        instruction: u64,
        operation: SelectionOperation,
        chooser: Role,
        domain: Vec<ObjectRef>,
        required_count: RulesInteger,
        selected: Vec<ObjectRef>,
        probes: Vec<SelectionProbeDeclaration>,
    },
    ResolutionGraveyardOrder {
        id: String,
        entry: EntryRef,
        instruction: u64,
        operation: SelectionOperation,
        owner: Role,
        domain: Vec<ObjectRef>,
        entrant_count: RulesInteger,
        entrants_top_first: Vec<ObjectRef>,
        probes: Vec<SelectionProbeDeclaration>,
    },
}

// Meaningful closed symbolic commands, even when the pinned positive inputs use [].
// Runtime authorities are supplied only by the maintained finite controller.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum SelectionProbeDeclaration {
    Execute {
        obligation: ObligationRef,
        basis: ProbeBasisRef,
        actor: Role,
        attempt: Box<SelectionProbeAttempt>,
        rejection: SelectionRejection,
    },
    PendingExactOriginalHistory {
        obligation: OriginalObligationRef,
        reason: PendingReason,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum ObligationRef {
    Original { id: OriginalObligationId },
    Companion { id: CompanionObligationId },
    Supplemental { id: SupplementalObligationId },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum OriginalObligationRef {
    Original { id: OriginalObligationId },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(in crate::ordinary_game) enum OriginalObligationId {
    #[serde(rename = "SD04-V1")]
    Sd04V1,
    #[serde(rename = "SD04-V2")]
    Sd04V2,
    #[serde(rename = "SD04-V3")]
    Sd04V3,
    #[serde(rename = "SD04-V4")]
    Sd04V4,
    #[serde(rename = "SD04-V5")]
    Sd04V5,
    #[serde(rename = "SD04-V6")]
    Sd04V6,
    #[serde(rename = "SD04-V7")]
    Sd04V7,
    #[serde(rename = "SD04-V8")]
    Sd04V8,
    #[serde(rename = "SD04-V9")]
    Sd04V9,
    #[serde(rename = "SD04-V10")]
    Sd04V10,
    #[serde(rename = "SD04-V11")]
    Sd04V11,
    #[serde(rename = "SD04-V12")]
    Sd04V12,
    #[serde(rename = "SD04-V13")]
    Sd04V13,
    #[serde(rename = "SD04-V14")]
    Sd04V14,
    #[serde(rename = "SD14-V1")]
    Sd14V1,
    #[serde(rename = "SD14-V2")]
    Sd14V2,
    #[serde(rename = "SD14-V3")]
    Sd14V3,
    #[serde(rename = "SD14-V4")]
    Sd14V4,
    #[serde(rename = "SD14-V5")]
    Sd14V5,
    #[serde(rename = "SD14-V6")]
    Sd14V6,
    #[serde(rename = "SD14-V7")]
    Sd14V7,
    #[serde(rename = "SD14-V8")]
    Sd14V8,
    #[serde(rename = "SD14-V9")]
    Sd14V9,
    #[serde(rename = "SD14-V10")]
    Sd14V10,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(in crate::ordinary_game) enum CompanionObligationId {
    #[serde(rename = "COMPANION-SD04-GENUINE-CONSUMED-D-OLD")]
    Sd04Consumed,
    #[serde(rename = "COMPANION-SD04-GENUINE-OBSOLETE-OBJECT")]
    Sd04Obsolete,
    #[serde(rename = "COMPANION-SD14-GENUINE-CONSUMED-D-OLD")]
    Sd14Consumed,
    #[serde(rename = "COMPANION-SD14-GENUINE-OBSOLETE-OBJECT")]
    Sd14Obsolete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ordinary_game) enum SupplementalObligationId {
    SelectionWrongReplyKind,
    OrderDuplicateMember,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ordinary_game) enum PendingReason {
    OriginalPrefixHasNoConsumedDecision,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(in crate::ordinary_game) enum ForeignHelper {
    #[serde(rename = "sd11_owner_order_v1")]
    Sd11OwnerOrderV1,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum ProbeBasisRef {
    Current {},
    Initializer {},
    BeforeConsumedPlan { plan: String },
    AfterConsumedPlan { plan: String },
    ForeignPendingHelper { helper: ForeignHelper },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum ProbeDecisionRef {
    Current {},
    ConsumedPlan { plan: String },
    ForeignPendingHelper { helper: ForeignHelper },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum SelectionProbeAttempt {
    Reply {
        decision: ProbeDecisionRef,
        choice: SelectionProbeReply,
    },
    Cast {
        spell: ObjectRef,
        target: Target,
    },
    ActivateMana {
        source: ObjectRef,
        ability: Ability,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ordinary_game) enum SelectionProbeReply {
    SelectResolution { objects: Vec<ObjectRef> },
    OrderResolutionGraveyard { entrants_top_first: Vec<ObjectRef> },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::ordinary_game) enum SelectionRejection {
    WrongGame,
    StaleRevision,
    WrongDecision,
    WrongChooser,
    DecisionRequired,
    InvalidSelectionCount,
    DuplicateSelection,
    SelectionOutsideDomain,
    InvalidGraveyardPermutation,
    WrongDecisionKind,
}

impl OriginalObligationId {
    pub(in crate::ordinary_game) fn as_str(self) -> &'static str {
        match self {
            Self::Sd04V1 => "SD04-V1",
            Self::Sd04V2 => "SD04-V2",
            Self::Sd04V3 => "SD04-V3",
            Self::Sd04V4 => "SD04-V4",
            Self::Sd04V5 => "SD04-V5",
            Self::Sd04V6 => "SD04-V6",
            Self::Sd04V7 => "SD04-V7",
            Self::Sd04V8 => "SD04-V8",
            Self::Sd04V9 => "SD04-V9",
            Self::Sd04V10 => "SD04-V10",
            Self::Sd04V11 => "SD04-V11",
            Self::Sd04V12 => "SD04-V12",
            Self::Sd04V13 => "SD04-V13",
            Self::Sd04V14 => "SD04-V14",
            Self::Sd14V1 => "SD14-V1",
            Self::Sd14V2 => "SD14-V2",
            Self::Sd14V3 => "SD14-V3",
            Self::Sd14V4 => "SD14-V4",
            Self::Sd14V5 => "SD14-V5",
            Self::Sd14V6 => "SD14-V6",
            Self::Sd14V7 => "SD14-V7",
            Self::Sd14V8 => "SD14-V8",
            Self::Sd14V9 => "SD14-V9",
            Self::Sd14V10 => "SD14-V10",
        }
    }
}

impl CompanionObligationId {
    pub(in crate::ordinary_game) fn as_str(self) -> &'static str {
        match self {
            Self::Sd04Consumed => "COMPANION-SD04-GENUINE-CONSUMED-D-OLD",
            Self::Sd04Obsolete => "COMPANION-SD04-GENUINE-OBSOLETE-OBJECT",
            Self::Sd14Consumed => "COMPANION-SD14-GENUINE-CONSUMED-D-OLD",
            Self::Sd14Obsolete => "COMPANION-SD14-GENUINE-OBSOLETE-OBJECT",
        }
    }
}

impl SupplementalObligationId {
    pub(in crate::ordinary_game) fn as_str(self) -> &'static str {
        match self {
            Self::SelectionWrongReplyKind => "selection_wrong_reply_kind",
            Self::OrderDuplicateMember => "order_duplicate_member",
        }
    }
}

impl ObligationRef {
    pub(in crate::ordinary_game) fn as_str(&self) -> &'static str {
        match self {
            Self::Original { id } => id.as_str(),
            Self::Companion { id } => id.as_str(),
            Self::Supplemental { id } => id.as_str(),
        }
    }
}
impl OriginalObligationRef {
    pub(in crate::ordinary_game) fn as_str(&self) -> &'static str {
        match self {
            Self::Original { id } => id.as_str(),
        }
    }
}
