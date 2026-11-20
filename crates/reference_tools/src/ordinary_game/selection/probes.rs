//! Pure symbolic probe lowering over genuine retained authorities. No native call lives here.
use super::{authority::ForeignAuthority, data::*};
use crate::ordinary_game::mandatory::data::{Appearance, EntryRef};
use crate::{
    ordinary_game::{
        mandatory::data::{Ability, ObjectRef, Target},
        probes::RejectionProbe,
    },
    packet,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use spellwake_contracts::creature_types::CreatureTypeId;
use spellwake_engine::{
    effects::{
        EffectError, ResolutionSelectionOperation, ResolutionSelectionStage,
        ResolutionSelectionState,
    },
    game::{
        DecisionId, DecisionReply, DecisionState, GameBasis, GameCommand, GameError, GameInput,
    },
    mana::ManaAbilityRef,
    object_identity::{CardId, GameObject, ObjectId},
    occurrence::{EventId, OccurrenceId, SupportedAppearance, TriggerInstanceId},
    priority::SeatId,
    program::CreatureTypeSlot,
    stack_window::StackEntryId,
    targets::TargetRef,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attachments {
    kind: String,
    schema_version: u32,
    status: String,
    attachments: Vec<Attachment>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attachment {
    case: SelectionCase,
    positive_plan: String,
    probes: Vec<SelectionProbeDeclaration>,
}
pub(super) fn validate_attachment(
    case: SelectionCase,
    plan: &str,
    probes: &[SelectionProbeDeclaration],
) -> Result<(), String> {
    let bytes = include_bytes!("probe-declarations-v1.json");
    if format!("{:x}", Sha256::digest(bytes))
        != "755aa2ce6e6db79cb6bc896304043b9c4ff44f63804b3b2aef1825ccbb34d8fe"
    {
        return Err("compiled selection probe contract authority mismatch".into());
    }
    let raw = packet::unique_json(bytes)?;
    let contract: Attachments = serde_json::from_value(raw)
        .map_err(|error| format!("selection probe contract schema: {error}"))?;
    if contract.schema_version != 1
        || contract.kind
            != "PROPOSED_PLAN_PROBES_ATTACHMENTS_NOT_A_SECOND_INPUTS_OR_GAMEINPUT_CODEC"
        || contract.status != "NOT_IMPLEMENTED_NOT_RUN"
    {
        return Err("selection probe contract header mismatch".into());
    }
    let expected = contract
        .attachments
        .iter()
        .find(|a| a.case == case && a.positive_plan == plan)
        .map_or(&[][..], |a| a.probes.as_slice());
    if probes != expected {
        return Err("selection compiled probe template mismatch".into());
    }
    Ok(())
}

/// Owned values copied from the existing public native resolution observation.
/// This is privileged annotation evidence, never a resumable state or a codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct SelectionWorkBinding {
    pub entry: StackEntryId,
    pub instruction: usize,
    pub locals: BTreeMap<CreatureTypeSlot, CreatureTypeId>,
    pub operation: ResolutionSelectionOperation,
    pub stage: ResolutionSelectionStage,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct ReachedSelection {
    pub case: SelectionCase,
    pub plan_id: String,
    pub index: usize,
    pub decision: DecisionState,
    pub work: SelectionWorkBinding,
}
impl ReachedSelection {
    /// Call only after the maintained selected matcher validates this exact work.
    pub(in crate::ordinary_game) fn capture(
        case: SelectionCase,
        plan_id: &str,
        index: usize,
        decision: &DecisionState,
        work: ResolutionSelectionState<'_>,
    ) -> Arc<Self> {
        Arc::new(Self {
            case,
            plan_id: plan_id.into(),
            index,
            decision: decision.clone(),
            work: SelectionWorkBinding {
                entry: work.cursor.entry(),
                instruction: work.cursor.instruction(),
                locals: work.cursor.locals().clone(),
                operation: work.operation.clone(),
                stage: work.stage.clone(),
            },
        })
    }
}
/// Actual committed-input coordinates from the existing finite accepted counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct AcceptedSelectionReceipt {
    pub ordinal: u64,
    pub input: GameInput,
    pub after_basis: GameBasis,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct ConsumedPlan {
    pub reached: Arc<ReachedSelection>,
    pub accepted: AcceptedSelectionReceipt,
}
impl ConsumedPlan {
    pub(in crate::ordinary_game) fn index(&self) -> usize {
        self.reached.index
    }
    pub(in crate::ordinary_game) fn decision(&self) -> &DecisionState {
        &self.reached.decision
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct SavedObjectAuthority {
    pub basis: GameBasis,
    pub instruction: String,
    pub object: GameObject,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct BirthAuthority {
    pub entry: StackEntryId,
    pub instruction: usize,
    pub creator: SeatId,
    pub member: usize,
    pub event: EventId,
    pub occurrence: OccurrenceId,
    pub appearance: SupportedAppearance,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) enum CausalAppearanceLink {
    Occurrence {
        event: EventId,
        occurrence: OccurrenceId,
        appearance: SupportedAppearance,
    },
    TriggerCapture {
        event: EventId,
        instance: TriggerInstanceId,
        appearance: SupportedAppearance,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct CausalObjectAuthority {
    pub cause: String,
    pub appearance: Appearance,
    pub object: ObjectId,
    pub links: Vec<CausalAppearanceLink>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) enum ObjectOrigin {
    CurrentPhysical {
        card: CardId,
        actual: GameObject,
    },
    Saved {
        binding: String,
        authority: SavedObjectAuthority,
    },
    PhysicalCause {
        card: CardId,
        authority: CausalObjectAuthority,
    },
    CurrentToken {
        birth: BirthAuthority,
        current: GameObject,
        lineage_birth: ObjectId,
    },
    TokenCause {
        birth: BirthAuthority,
        authority: CausalObjectAuthority,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct ResolvedObject {
    pub reference: ObjectRef,
    pub object: ObjectId,
    pub origin: Box<ObjectOrigin>,
}
impl ResolvedObject {
    pub(in crate::ordinary_game) fn actual_object(&self) -> Result<&GameObject, String> {
        match self.origin.as_ref() {
            ObjectOrigin::CurrentPhysical { actual, .. }
            | ObjectOrigin::CurrentToken {
                current: actual, ..
            } => Ok(actual),
            ObjectOrigin::Saved { authority, .. } => Ok(&authority.object),
            ObjectOrigin::PhysicalCause { authority, .. }
            | ObjectOrigin::TokenCause { authority, .. } => authority
                .links
                .first()
                .map(|link| match link {
                    CausalAppearanceLink::Occurrence { appearance, .. }
                    | CausalAppearanceLink::TriggerCapture { appearance, .. } => &appearance.object,
                })
                .ok_or_else(|| "resolved causal object lacks its actual appearance link".into()),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct ResolvedEntry {
    pub reference: EntryRef,
    pub entry: StackEntryId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) enum ResolvedTarget {
    Object(ResolvedObject),
    Stack(ResolvedEntry),
    Player {
        role: crate::ordinary_game::mandatory::data::Role,
        player: SeatId,
    },
}
impl ResolvedTarget {
    pub(in crate::ordinary_game) fn native(&self) -> TargetRef {
        match self {
            Self::Object(r) => TargetRef::Object(r.object),
            Self::Stack(r) => TargetRef::Stack(r.entry),
            Self::Player { player, .. } => TargetRef::Player(*player),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) enum ResolvedAttempt {
    Reply {
        decision: DecisionId,
        choice: ResolvedReply,
    },
    Cast {
        spell: ResolvedObject,
        target: ResolvedTarget,
    },
    ActivateMana {
        source: ResolvedObject,
        ability: ManaAbilityRef,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) enum ResolvedReply {
    SelectResolution(Vec<ResolvedObject>),
    OrderResolutionGraveyard(Vec<ResolvedObject>),
}
impl ResolvedAttempt {
    fn command(&self) -> GameCommand {
        match self {
            Self::Reply { decision, choice } => GameCommand::Reply {
                decision: *decision,
                choice: match choice {
                    ResolvedReply::SelectResolution(rows) => {
                        DecisionReply::SelectResolution(rows.iter().map(|r| r.object).collect())
                    }
                    ResolvedReply::OrderResolutionGraveyard(rows) => {
                        DecisionReply::OrderResolutionGraveyard(
                            rows.iter().map(|r| r.object).collect(),
                        )
                    }
                },
            },
            Self::Cast { spell, target } => GameCommand::Cast {
                spell: spell.object,
                target: target.native(),
            },
            Self::ActivateMana { source, ability } => GameCommand::ActivateMana {
                source: source.object,
                ability: *ability,
            },
        }
    }
}
/// Borrowed privileged prepared evidence, not seat-safe or host acceptance proof.
#[derive(Debug, Clone, Copy)]
pub(in crate::ordinary_game) struct PreparedSelectionMetadata<'a> {
    pub reached: &'a Arc<ReachedSelection>,
    pub receipts: &'a [ProbeAuthorityReceipt],
    pub pending: &'a [PendingObligation],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct ConsumedAuthorityLink {
    pub plan_id: String,
    pub reached: Arc<ReachedSelection>,
    pub accepted: AcceptedSelectionReceipt,
}
/// Construction credit and exact requested history are deliberately independent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) enum FulfillmentDisposition {
    OriginalConstructed,
    OriginalCurrentGraveyardOnly {
        exact_obsolete_history: bool,
    },
    OriginalInitializerBasisOnly {
        exact_earlier_resolution_of_same_source: bool,
    },
    CompanionSeparateHistory {
        original_retrocredit: bool,
    },
    Supplemental,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct ProbeAuthorityReceipt {
    pub obligation: ObligationRef,
    pub reached: Arc<ReachedSelection>,
    pub probe: RejectionProbe,
    pub attempted: ResolvedAttempt,
    pub consumed_links: Vec<ConsumedAuthorityLink>,
    pub foreign_binding: Option<Arc<ReachedSelection>>,
    pub basis_source: ProbeBasisRef,
    pub basis: GameBasis,
    pub decision_source: Option<ProbeDecisionRef>,
    pub decision: Option<DecisionState>,
    pub fulfillment: FulfillmentDisposition,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct PendingObligation {
    pub obligation: OriginalObligationRef,
    pub reached: Arc<ReachedSelection>,
    pub reason: PendingReason,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(in crate::ordinary_game) struct PreparedProbeBatch {
    pub probes: Vec<RejectionProbe>,
    pub receipts: Vec<ProbeAuthorityReceipt>,
    pub pending: Vec<PendingObligation>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) enum ProbePreparationError {
    DuplicateObligation(String),
    MissingInitializerAuthority,
    MissingConsumedPlan(String),
    InvalidForeignHelper,
    CollidingForeignDecision,
    ObjectAuthority(String),
    TargetAuthority(String),
    AbilityAuthority(String),
}
impl std::fmt::Display for ProbePreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "selected probe authority preparation: {self:?}")
    }
}
impl std::error::Error for ProbePreparationError {}

impl SelectionRejection {
    fn native(self) -> GameError {
        match self {
            Self::WrongGame => GameError::WrongGame,
            Self::StaleRevision => GameError::StaleRevision,
            Self::WrongDecision => GameError::WrongDecision,
            Self::WrongChooser => GameError::WrongChooser,
            Self::DecisionRequired => GameError::DecisionRequired,
            Self::InvalidSelectionCount => GameError::Effect(EffectError::InvalidSelectionCount),
            Self::DuplicateSelection => GameError::Effect(EffectError::DuplicateSelection),
            Self::SelectionOutsideDomain => GameError::Effect(EffectError::SelectionOutsideDomain),
            Self::InvalidGraveyardPermutation => {
                GameError::Effect(EffectError::InvalidGraveyardPermutation)
            }
            Self::WrongDecisionKind => GameError::Effect(EffectError::WrongDecisionKind),
        }
    }
}
fn fulfillment(obligation: &ObligationRef) -> FulfillmentDisposition {
    match obligation {
        ObligationRef::Original {
            id: OriginalObligationId::Sd04V8 | OriginalObligationId::Sd14V6,
        } => FulfillmentDisposition::OriginalCurrentGraveyardOnly {
            exact_obsolete_history: false,
        },
        ObligationRef::Original {
            id: OriginalObligationId::Sd04V11,
        } => FulfillmentDisposition::OriginalInitializerBasisOnly {
            exact_earlier_resolution_of_same_source: false,
        },
        ObligationRef::Original { .. } => FulfillmentDisposition::OriginalConstructed,
        ObligationRef::Companion { .. } => FulfillmentDisposition::CompanionSeparateHistory {
            original_retrocredit: false,
        },
        ObligationRef::Supplemental { .. } => FulfillmentDisposition::Supplemental,
    }
}
/// Borrowed exact accepted authorities; no field creates a native identity.
pub(in crate::ordinary_game) struct ProbeAuthorities<'a> {
    pub reached: &'a Arc<ReachedSelection>,
    pub initializer: Option<&'a GameBasis>,
    pub consumed: &'a BTreeMap<String, ConsumedPlan>,
    pub foreign: Option<&'a ForeignAuthority>,
}
impl ProbeAuthorities<'_> {
    fn consumed(&self, id: &str) -> Result<&ConsumedPlan, ProbePreparationError> {
        self.consumed
            .get(id)
            .ok_or_else(|| ProbePreparationError::MissingConsumedPlan(id.into()))
    }
    fn foreign(&self) -> Result<&ForeignAuthority, ProbePreparationError> {
        let foreign = self
            .foreign
            .ok_or(ProbePreparationError::InvalidForeignHelper)?;
        if foreign.basis().game == self.reached.decision.basis().game
            || !matches!(
                foreign.decision().frame(),
                spellwake_engine::game::RuleFrame::ResolutionGraveyardOrder(_)
            )
        {
            return Err(ProbePreparationError::InvalidForeignHelper);
        }
        if foreign.decision().id() == self.reached.decision.id() {
            return Err(ProbePreparationError::CollidingForeignDecision);
        }
        Ok(foreign)
    }
    fn basis(&self, reference: &ProbeBasisRef) -> Result<GameBasis, ProbePreparationError> {
        Ok(match reference {
            ProbeBasisRef::Current {} => self.reached.decision.basis().clone(),
            ProbeBasisRef::Initializer {} => self
                .initializer
                .ok_or(ProbePreparationError::MissingInitializerAuthority)?
                .clone(),
            ProbeBasisRef::BeforeConsumedPlan { plan } => {
                self.consumed(plan)?.accepted.input.basis.clone()
            }
            ProbeBasisRef::AfterConsumedPlan { plan } => {
                self.consumed(plan)?.accepted.after_basis.clone()
            }
            ProbeBasisRef::ForeignPendingHelper { .. } => self.foreign()?.basis().clone(),
        })
    }
    fn decision(
        &self,
        reference: &ProbeDecisionRef,
    ) -> Result<DecisionState, ProbePreparationError> {
        Ok(match reference {
            ProbeDecisionRef::Current {} => self.reached.decision.clone(),
            ProbeDecisionRef::ConsumedPlan { plan } => self.consumed(plan)?.decision().clone(),
            ProbeDecisionRef::ForeignPendingHelper { .. } => self.foreign()?.decision().clone(),
        })
    }
}
/// Lower each immutable attempted vector unchanged after the positive frame guards.
/// No advance, initialize, replay, annotation consumption or repair occurs here.
pub(in crate::ordinary_game) fn lower(
    declarations: &[SelectionProbeDeclaration],
    authorities: ProbeAuthorities<'_>,
    object: impl Fn(&ObjectRef) -> Result<ResolvedObject, String>,
    target: impl Fn(&Target) -> Result<ResolvedTarget, String>,
    ability: impl Fn(&Ability) -> Result<ManaAbilityRef, String>,
) -> Result<PreparedProbeBatch, ProbePreparationError> {
    let mut batch = PreparedProbeBatch::default();
    let mut seen = BTreeSet::new();
    for declaration in declarations {
        let id = match declaration {
            SelectionProbeDeclaration::Execute { obligation, .. } => obligation.as_str(),
            SelectionProbeDeclaration::PendingExactOriginalHistory { obligation, .. } => {
                obligation.as_str()
            }
        };
        if !seen.insert(id) {
            return Err(ProbePreparationError::DuplicateObligation(id.into()));
        }
        match declaration {
            SelectionProbeDeclaration::PendingExactOriginalHistory { obligation, reason } => {
                batch.pending.push(PendingObligation {
                    obligation: obligation.clone(),
                    reached: authorities.reached.clone(),
                    reason: *reason,
                })
            }
            SelectionProbeDeclaration::Execute {
                obligation,
                basis,
                actor,
                attempt,
                rejection,
            } => {
                let actual_basis = authorities.basis(basis)?;
                let objects = |rows: &[ObjectRef]| {
                    rows.iter()
                        .map(|r| object(r).map_err(ProbePreparationError::ObjectAuthority))
                        .collect::<Result<Vec<_>, _>>()
                };
                let (attempted, decision_source, actual_decision) = match attempt.as_ref() {
                    SelectionProbeAttempt::Reply { decision, choice } => {
                        let actual = authorities.decision(decision)?;
                        let choice = match choice {
                            SelectionProbeReply::SelectResolution { objects: refs } => {
                                ResolvedReply::SelectResolution(objects(refs)?)
                            }
                            SelectionProbeReply::OrderResolutionGraveyard {
                                entrants_top_first,
                            } => ResolvedReply::OrderResolutionGraveyard(objects(
                                entrants_top_first,
                            )?),
                        };
                        (
                            ResolvedAttempt::Reply {
                                decision: actual.id(),
                                choice,
                            },
                            Some(decision.clone()),
                            Some(actual),
                        )
                    }
                    SelectionProbeAttempt::Cast {
                        spell,
                        target: reference,
                    } => (
                        ResolvedAttempt::Cast {
                            spell: object(spell).map_err(ProbePreparationError::ObjectAuthority)?,
                            target: target(reference)
                                .map_err(ProbePreparationError::TargetAuthority)?,
                        },
                        None,
                        None,
                    ),
                    SelectionProbeAttempt::ActivateMana {
                        source,
                        ability: reference,
                    } => (
                        ResolvedAttempt::ActivateMana {
                            source: object(source)
                                .map_err(ProbePreparationError::ObjectAuthority)?,
                            ability: ability(reference)
                                .map_err(ProbePreparationError::AbilityAuthority)?,
                        },
                        None,
                        None,
                    ),
                };
                let mut consumed_links = vec![];
                let mut add_consumed = |name: &str| -> Result<(), ProbePreparationError> {
                    if !consumed_links
                        .iter()
                        .any(|r: &ConsumedAuthorityLink| r.plan_id == name)
                    {
                        let consumed = authorities.consumed(name)?;
                        consumed_links.push(ConsumedAuthorityLink {
                            plan_id: name.into(),
                            reached: consumed.reached.clone(),
                            accepted: consumed.accepted.clone(),
                        });
                    }
                    Ok(())
                };
                if let ProbeBasisRef::BeforeConsumedPlan { plan }
                | ProbeBasisRef::AfterConsumedPlan { plan } = basis
                {
                    add_consumed(plan)?;
                }
                if let Some(ProbeDecisionRef::ConsumedPlan { plan }) = &decision_source {
                    add_consumed(plan)?;
                }
                let foreign_binding = if matches!(basis, ProbeBasisRef::ForeignPendingHelper { .. })
                    || matches!(
                        decision_source,
                        Some(ProbeDecisionRef::ForeignPendingHelper { .. })
                    ) {
                    Some(authorities.foreign()?.binding().clone())
                } else {
                    None
                };
                let probe = RejectionProbe {
                    id: obligation.as_str().into(),
                    expectation_ref: obligation.as_str().into(),
                    attachment: authorities.reached.plan_id.clone(),
                    input: GameInput {
                        basis: actual_basis.clone(),
                        actor: actor.seat(),
                        command: attempted.command(),
                    },
                    expected: rejection.native(),
                };
                batch.probes.push(probe.clone());
                batch.receipts.push(ProbeAuthorityReceipt {
                    obligation: obligation.clone(),
                    reached: authorities.reached.clone(),
                    probe,
                    attempted,
                    consumed_links,
                    foreign_binding,
                    basis_source: basis.clone(),
                    basis: actual_basis,
                    decision_source,
                    decision: actual_decision,
                    fulfillment: fulfillment(obligation),
                });
            }
        }
    }
    Ok(batch)
}
