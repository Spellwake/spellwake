//! Typed privileged observations of actual native candidates. These values are
//! diagnostic evidence, never a second rules state or a public player view.
use super::fixture::{A, B, Fixture, ObservationScope, ReportEpoch};
use crate::current_profile::applicability::NativeCaptureValue;
use crate::{check_trace, damage_trace, program, projection};
use serde::Serialize;
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    combat::{
        AssignmentOrigin, BlockStatus, CombatDecision, CombatDeclarationRecord, DeclarationOrigin,
    },
    draw::{DrawEvent, GameOutcome},
    effects::{CopyTargetChoice, DecisionReply},
    game::{
        CandidateBase, GameBasis, GameCommand, GameInput, GameState, NativeCandidate, RuleFrame,
    },
    mana::{ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, GameObject, ObjectId, ObjectZone},
    occurrence::{RulesJournal, RulesOccurrence, RulesRecord},
    opening::{MulliganChoice, OpeningDecision},
    priority::PriorityPhase,
    stack_window::{StackKind, WindowPhase},
    targets::TargetRef,
    turn::{TurnStage, TurnWork},
};
use std::collections::BTreeMap;

#[cfg(test)]
mod concession_tests;
mod epoch;
#[cfg(test)]
mod library_capability_tests;
use epoch::{ExecutionObservation, HeaderEpoch, JournalEpoch};

/// Capability is checked over actual native values, independently of a caller's
/// mutable Fixture/provenance equality. Generic diagnostic codecs are broader.
fn check_predecessor_semantics(value: &impl NativeCaptureValue) -> Result<(), String> {
    value.check_current10().map_err(|_| {
        "ordinary predecessor projection excludes unrepresented native capabilities".to_owned()
    })
}
pub(super) fn opaque(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}
pub(super) fn alias(fixture: &Fixture, card: CardId) -> Result<String, String> {
    fixture
        .aliases
        .get(&card)
        .map(|row| row.alias.clone())
        .ok_or_else(|| format!("unbound physical card {}", card.0))
}

/// Exact historical incarnations are retained only for evidence annotation.
#[derive(Clone, Default)]
pub(super) struct History(BTreeMap<ObjectId, GameObject>);
impl History {
    pub fn remember(&mut self, source: Option<&GameState>, candidate: &NativeCandidate) {
        if let Some(source) = source {
            for card in source.execution().window().cards().iter() {
                self.insert(card);
            }
        }
        for card in candidate.state().execution().window().cards().iter() {
            self.insert(card);
        }
        for row in &candidate.draw_journal().events {
            if let DrawEvent::Drawn { before, after, .. } = row {
                self.insert(before);
                self.insert(after);
            }
        }
        for row in &candidate.check_journal().moves {
            self.insert(&row.before);
            self.insert(&row.after);
        }
        if let Some(row) = candidate.land_play() {
            self.insert(&row.before);
            self.insert(&row.after);
        }
        if let Some(row) = candidate.permanent_entry() {
            self.insert(&row.before);
            self.insert(&row.after);
        }
        // Endpoint inventories omit objects born and removed within one command.
        // Retain actual journal appearances, never synthesize an incarnation.
        for record in &candidate.rules_journal().records {
            match record {
                RulesRecord::Event(event) => {
                    for occurrence in &event.occurrences {
                        match &occurrence.value {
                            RulesOccurrence::TokenCreated { after, .. } => {
                                self.insert(&after.object)
                            }
                            RulesOccurrence::TokenCeased { object } => self.insert(object),
                            RulesOccurrence::ZoneChange(change) => {
                                self.insert(&change.before.object);
                                self.insert(&change.after.object);
                            }
                            RulesOccurrence::ControllerChanged(change) => {
                                self.insert(&change.before);
                                self.insert(&change.after);
                            }
                            RulesOccurrence::StatusChange { change, .. } => {
                                self.insert(&change.before);
                                self.insert(&change.after);
                            }
                            RulesOccurrence::CastCompleted(cast) => {
                                self.insert(&cast.spell().object)
                            }
                            RulesOccurrence::ManaAbility { source, .. }
                            | RulesOccurrence::ActivatedAbility { source, .. } => {
                                self.insert(source)
                            }
                            _ => {}
                        }
                    }
                }
                RulesRecord::TriggerDiscovered(instance) => self.insert(&instance.source.object),
                _ => {}
            }
        }
    }
    fn insert(&mut self, card: &GameObject) {
        self.0.insert(card.id(), card.clone());
    }
    pub fn exact(&self, fixture: &Fixture, id: ObjectId) -> Result<ExactRef, String> {
        Ok(ExactRef {
            object: opaque(id),
            alias: self
                .0
                .get(&id)
                .and_then(GameObject::card_id)
                .map(|card| alias(fixture, card))
                .transpose()?,
        })
    }
    /// Require a real remembered card or token, independently of physical aliases.
    pub fn known(&self, fixture: &Fixture, id: ObjectId) -> Result<ExactRef, String> {
        if !self.0.contains_key(&id) {
            return Err(format!("missing exact historical object {id:?}"));
        }
        self.exact(fixture, id)
    }
    /// Require a physical card for ordered card piles and legacy spell payloads.
    pub fn required(&self, fixture: &Fixture, id: ObjectId) -> Result<ExactRef, String> {
        let value = self.exact(fixture, id)?;
        if value.alias.is_none() {
            return Err(format!("missing exact historical object {id:?}"));
        }
        Ok(value)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ExactRef {
    pub object: String,
    pub alias: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct BasisView {
    pub game: String,
    pub context_ref: String,
    pub epoch: String,
    pub revision: String,
}
pub(super) fn basis(fixture: &Fixture, value: &GameBasis) -> Result<BasisView, String> {
    fixture.require_ordinary_execution()?;
    if value.context != fixture.context {
        return Err("actual native context differs from fixture-owned constructor context".into());
    }
    Ok(BasisView {
        game: value.game.0.to_string(),
        context_ref: fixture.context_inputs.reference.clone(),
        epoch: value.epoch.to_string(),
        revision: value.revision.to_string(),
    })
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum BaseView {
    Initialize { game: String, context_ref: String },
    Advance { basis: BasisView },
}
pub(super) fn base(fixture: &Fixture, value: &CandidateBase) -> Result<BaseView, String> {
    fixture.require_ordinary_execution()?;
    match value {
        CandidateBase::Initialize { game, context } => {
            if *context != fixture.context {
                return Err("initial context mismatch".into());
            }
            Ok(BaseView::Initialize {
                game: game.0.to_string(),
                context_ref: fixture.context_inputs.reference.clone(),
            })
        }
        CandidateBase::Advance(value) => Ok(BaseView::Advance {
            basis: basis(fixture, value)?,
        }),
    }
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct InputView {
    pub basis: BasisView,
    pub actor: u32,
    pub command: CommandView,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum TargetView {
    Card { object: ExactRef },
    Player { player: u32 },
    Stack { entry: String },
}
fn target(fixture: &Fixture, history: &History, value: TargetRef) -> Result<TargetView, String> {
    Ok(match value {
        TargetRef::Object(id) => TargetView::Card {
            object: history.exact(fixture, id)?,
        },
        TargetRef::Player(player) => TargetView::Player { player: player.0 },
        TargetRef::Stack(entry) => TargetView::Stack {
            entry: opaque(entry),
        },
    })
}
pub(super) fn mana_type(value: ManaType) -> &'static str {
    match value {
        ManaType::White => "white",
        ManaType::Blue => "blue",
        ManaType::Black => "black",
        ManaType::Red => "red",
        ManaType::Green => "green",
        ManaType::Colourless => "colorless",
    }
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct SpendView {
    pub mana_type: &'static str,
    pub amount: RulesInteger,
}
fn spend(values: &[ManaSpend]) -> Vec<SpendView> {
    values
        .iter()
        .map(|v| SpendView {
            mana_type: mana_type(v.mana_type),
            amount: v.amount.clone(),
        })
        .collect()
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct PaymentView {
    pub spend: Vec<SpendView>,
}
fn payment(value: &ManaPayment) -> PaymentView {
    PaymentView {
        spend: spend(&value.spend),
    }
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct CostView {
    pub generic: RulesInteger,
    pub required: BTreeMap<&'static str, RulesInteger>,
}
fn cost(value: &ManaCost) -> CostView {
    CostView {
        generic: value.generic.clone(),
        required: value
            .required
            .iter()
            .map(|(t, n)| (mana_type(*t), n.clone()))
            .collect(),
    }
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct AttackView {
    pub attacker: ExactRef,
    pub defender: u32,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct BlockView {
    pub blocker: ExactRef,
    pub attacker: ExactRef,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct AssignmentView {
    pub source: ExactRef,
    pub target: damage_trace::DamageRecipientView,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ReplyView {
    OptionalEffect {
        choice: &'static str,
    },
    ResolutionPayment {
        choice: ResolutionPaymentChoiceView,
    },
    TriggerTarget {
        target: TargetView,
    },
    TimestampOrder {
        ordered: Vec<usize>,
    },
    Discard {
        ordered: Vec<ExactRef>,
    },
    OrderGraveyard {
        ordered: Vec<ExactRef>,
    },
    KeepLegend {
        conflict: check_trace::LegendConflictIdView,
        keep: ExactRef,
    },
    OrderTriggers {
        ordered: Vec<String>,
    },
    Bottom {
        ordered: Vec<ExactRef>,
    },
    Mulligan {
        choice: &'static str,
    },
    DeclareAttackers {
        attackers: Vec<AttackView>,
    },
    DeclareBlockers {
        blockers: Vec<BlockView>,
    },
    AssignCombatDamage {
        assignments: Vec<AssignmentView>,
    },
    CopyTargets {
        choice: CopyChoiceView,
    },
    DamageOrder {
        application: damage_trace::DamageApplicationView,
    },
    AllocatePrevention {
        allocations: Vec<PreventionChoiceView>,
    },
    ManaColor {
        mana_type: &'static str,
    },
    CreatureType {
        value: serde_json::Value,
    },
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ResolutionPaymentChoiceView {
    Pay { payment: PaymentView },
    Decline,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CopyChoiceView {
    Retain,
    Change { target: TargetView },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct PreventionChoiceView {
    attempt: String,
    amount: RulesInteger,
}
fn require_optional_scope(fixture: &Fixture, subject: &str) -> Result<(), String> {
    if fixture.case.observation_contract().observation_scope != ObservationScope::OptionalPayment {
        return Err(format!(
            "ordinary Report1–3 exclude resolution choice {subject}"
        ));
    }
    Ok(())
}
fn reply(fixture: &Fixture, history: &History, value: &DecisionReply) -> Result<ReplyView, String> {
    fixture.require_ordinary_execution()?;
    check_predecessor_semantics(value)?;
    let refs = |values: &[ObjectId]| {
        values
            .iter()
            .map(|id| history.exact(fixture, *id))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(match value {
        DecisionReply::PromptChoice(_) => {
            return Err("prompt choice exceeds historical scope".into());
        }
        DecisionReply::RemoveCounters(_) => {
            return Err("counter removal exceeds historical scope".into());
        }
        DecisionReply::Proliferate(_) => return Err("proliferate exceeds historical scope".into()),
        DecisionReply::PayAttackCost { .. }
        | DecisionReply::RedeclareReversingMana(_)
        | DecisionReply::RedeclareKeepingMana
        | DecisionReply::RedeclareAttackers => {
            return Err("attack costs exceed historical ordinary-game scope".into());
        }
        DecisionReply::ApplyCounterDoublersFirst(_) => {
            return Err("historical scope excludes counter replacement ordering".into());
        }
        DecisionReply::TriggerModes(_) => {
            return Err("historical scope excludes modal triggers".into());
        }
        DecisionReply::TriggerPermanentTargets(_) => {
            return Err("historical scope excludes multiple-target trigger selection".into());
        }
        DecisionReply::CastRecovery(_) => {
            return Err("historical ordinary scopes exclude cast recovery".into());
        }
        DecisionReply::EntryChoice(_)
        | DecisionReply::TokenEntryChoice(_)
        | DecisionReply::TokenAuraChoice(_)
        | DecisionReply::LinkedReturnOrder(_)
        | DecisionReply::LinkedAuraChoice(_) => {
            return Err("historical ordinary scopes exclude pre-entry choices".into());
        }
        DecisionReply::ZoneReplacement(_) => {
            return Err("ordinary observer does not admit zone replacement replies".into());
        }
        DecisionReply::LibraryTop(_) => {
            return Err("historical ordinary scopes exclude library choices".into());
        }
        DecisionReply::SelectResolution(_) | DecisionReply::OrderResolutionGraveyard(_) => {
            return Err(
                "ordinary historical observation scopes exclude resolution-selection replies"
                    .into(),
            );
        }
        DecisionReply::ImpulseCast { .. }
        | DecisionReply::CascadeCast { .. }
        | DecisionReply::MadnessCast { .. }
        | DecisionReply::ReboundCast { .. }
        | DecisionReply::SuspendCast { .. } => {
            return Err("historical ordinary scopes exclude triggered casting from exile".into());
        }
        DecisionReply::OptionalEffect(choice) => {
            require_optional_scope(fixture, "replies")?;
            ReplyView::OptionalEffect {
                choice: match choice {
                    spellwake_engine::effects::OptionalEffectChoice::Take => "take",
                    spellwake_engine::effects::OptionalEffectChoice::Decline => "decline",
                    spellwake_engine::effects::OptionalEffectChoice::PermanentTypeOrder(_)
                    | spellwake_engine::effects::OptionalEffectChoice::PermanentType(_) => {
                        return Err(
                            "historical ordinary fixtures exclude permanent type ordering".into(),
                        );
                    }
                },
            }
        }
        DecisionReply::ResolutionPayment(choice) => {
            require_optional_scope(fixture, "replies")?;
            ReplyView::ResolutionPayment {
                choice: match choice {
                    spellwake_engine::effects::ResolutionPaymentReply::Pay(value) => {
                        ResolutionPaymentChoiceView::Pay {
                            payment: payment(value),
                        }
                    }
                    spellwake_engine::effects::ResolutionPaymentReply::Decline => {
                        ResolutionPaymentChoiceView::Decline
                    }
                },
            }
        }
        DecisionReply::TriggerTarget(value) => {
            if !matches!(
                fixture.case.observation_contract().observation_scope,
                ObservationScope::Mandatory | ObservationScope::OptionalPayment
            ) {
                return Err(
                    "ordinary predecessor projection excludes trigger target replies".into(),
                );
            }
            ReplyView::TriggerTarget {
                target: target(fixture, history, *value)?,
            }
        }
        DecisionReply::TransformOrder(_) => {
            return Err("historical ordinary profile excludes transforming permanents".into());
        }
        DecisionReply::ChooseLandKind(_)
        | DecisionReply::MutateOnTop(_)
        | DecisionReply::ChoosePile(_)
        | DecisionReply::PlaneswalkerType(_)
        | DecisionReply::AttackDirection(_)
        | DecisionReply::DrawReplacement(_)
        | DecisionReply::ChooseDamageRecipient(_)
        | DecisionReply::ChooseDamageSource(_)
        | DecisionReply::OpponentChoice(_)
        | DecisionReply::ReplacementChooser(_)
        | DecisionReply::MyriadDefender(_)
        | DecisionReply::ReselectAttackDefender(_)
        | DecisionReply::TokenDefender(_) => {
            return Err("historical ordinary fixtures exclude attacking token choices".into());
        }
        DecisionReply::TimestampOrder(values) => ReplyView::TimestampOrder {
            ordered: values.iter().map(|v| v.ordinal()).collect(),
        },
        DecisionReply::CreatureType(value) => ReplyView::CreatureType {
            value: crate::projection::creature_type_id(*value),
        },
        DecisionReply::KeepLegend { conflict, keep } => ReplyView::KeepLegend {
            conflict: check_trace::legend_conflict_id_view(*conflict),
            keep: history.exact(fixture, *keep)?,
        },
        DecisionReply::Discard(v) => ReplyView::Discard { ordered: refs(v)? },
        DecisionReply::ReturnCommanders(_) => {
            return Err("historical ordinary scopes exclude commander choices".into());
        }
        DecisionReply::OrderGraveyard(v) => ReplyView::OrderGraveyard { ordered: refs(v)? },
        DecisionReply::OrderTriggers(v) => ReplyView::OrderTriggers {
            ordered: v.iter().map(|id| id.sequence().to_string()).collect(),
        },
        DecisionReply::Bottom(v) => ReplyView::Bottom { ordered: refs(v)? },
        DecisionReply::Mulligan(v) => ReplyView::Mulligan {
            choice: match v {
                MulliganChoice::Keep => "keep",
                MulliganChoice::Mulligan => "mulligan",
            },
        },
        DecisionReply::DeclareBandedAttackers { .. } => {
            return Err("historical ordinary scopes exclude bands".into());
        }
        DecisionReply::DeclareAttackers(v) => ReplyView::DeclareAttackers {
            attackers: v
                .iter()
                .map(|v| {
                    Ok(AttackView {
                        attacker: history.exact(fixture, v.attacker)?,
                        defender: v.defender.0,
                    })
                })
                .collect::<Result<_, String>>()?,
        },
        DecisionReply::DeclareBlockers(v) => ReplyView::DeclareBlockers {
            blockers: v
                .iter()
                .map(|v| {
                    Ok(BlockView {
                        blocker: history.exact(fixture, v.blocker)?,
                        attacker: history.exact(fixture, v.attacker)?,
                    })
                })
                .collect::<Result<_, String>>()?,
        },
        DecisionReply::AssignCombatDamage(v) => ReplyView::AssignCombatDamage {
            assignments: v
                .iter()
                .map(|v| {
                    Ok(AssignmentView {
                        source: history.exact(fixture, v.source)?,
                        target: damage_trace::damage_recipient_view(v.target),
                        amount: v.amount.clone(),
                    })
                })
                .collect::<Result<_, String>>()?,
        },
        DecisionReply::CopyTargets(v) => ReplyView::CopyTargets {
            choice: match v {
                CopyTargetChoice::Retain => CopyChoiceView::Retain,
                CopyTargetChoice::Change(v) => CopyChoiceView::Change {
                    target: target(fixture, history, *v)?,
                },
            },
        },
        DecisionReply::DamageOrder(v) => ReplyView::DamageOrder {
            application: damage_trace::damage_application_view(*v),
        },
        DecisionReply::AllocatePrevention(v) => ReplyView::AllocatePrevention {
            allocations: v
                .iter()
                .map(|v| PreventionChoiceView {
                    attempt: opaque(v.attempt),
                    amount: v.amount.clone(),
                })
                .collect(),
        },
        DecisionReply::SplitMana(_) => {
            return Err("historical scope excludes split mana allocation".into());
        }
        DecisionReply::OptionIndex(_) => {
            return Err("historical scope excludes option-index selection".into());
        }
        DecisionReply::UntapKeepTapped(_) | DecisionReply::OpeningBattlefield(_) => {
            return Err(
                "historical scope excludes untap-keep-tapped and opening-battlefield choices"
                    .into(),
            );
        }
        DecisionReply::ManaColor(v) => ReplyView::ManaColor {
            mana_type: mana_type(*v),
        },
    })
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CommandView {
    Pass,
    PlayLand {
        land: ExactRef,
    },
    ActivateMana {
        source: ExactRef,
        ability: serde_json::Value,
    },
    CastUntargeted {
        spell: ExactRef,
        payment: Option<PaymentView>,
    },
    Cast {
        spell: ExactRef,
        target: TargetView,
    },
    CastPaid {
        spell: ExactRef,
        target: TargetView,
        payment: PaymentView,
    },
    ActivateAbility {
        source: ExactRef,
        ability: String,
        target: TargetView,
        payment: Option<PaymentView>,
    },
    Reply {
        decision: String,
        choice: ReplyView,
    },
}
pub(super) fn input(
    fixture: &Fixture,
    history: &History,
    value: &GameInput,
) -> Result<InputView, String> {
    fixture.require_ordinary_execution()?;
    let observed_basis = if value.basis.context == fixture.context {
        basis(fixture, &value.basis)?
    } else {
        let (fields, context) = fixture.probe_context()?;
        if value.basis.context != context {
            return Err("attempted basis has no exact registered context".into());
        }
        BasisView {
            game: value.basis.game.0.to_string(),
            context_ref: fields.reference,
            epoch: value.basis.epoch.to_string(),
            revision: value.basis.revision.to_string(),
        }
    };
    Ok(InputView {
        basis: observed_basis,
        actor: value.actor.0,
        command: match &value.command {
            GameCommand::ActivateWithCostObjects { .. }
            | GameCommand::ActivateEmblem { .. }
            | GameCommand::ActivatePermanentTargets { .. }
            | GameCommand::ActivateModalAbility { .. }
            | GameCommand::ActivateGranted { .. } => {
                return Err(
                    "ordinary predecessor projection excludes granted activated abilities".into(),
                );
            }
            GameCommand::ActivateLoyaltyX { .. } => {
                return Err(
                    "ordinary predecessor projection excludes variable loyalty costs".into(),
                );
            }
            GameCommand::Foretell { .. }
            | GameCommand::Suspend { .. }
            | GameCommand::Plot { .. }
            | GameCommand::CastWithResources { .. } => {
                return Err(
                    "ordinary predecessor projection excludes casting with additional resources"
                        .into(),
                );
            }
            GameCommand::PlayLandWithEntryColor { .. }
            | GameCommand::PlayLandWithEntryReveal { .. } => {
                return Err("ordinary predecessor projection excludes pre-entry choices".into());
            }
            GameCommand::ActivateWithRestrictedMana { .. }
            | GameCommand::ActivateManaWithRestrictedPayment { .. }
            | GameCommand::ActivateHybridMana { .. }
            | GameCommand::ActivateManaWithPayment { .. }
            | GameCommand::ActivateManaWithBundle { .. }
            | GameCommand::ActivateManaWithCostObjects { .. }
            | GameCommand::ActivateManaForPlayer { .. } => {
                return Err("ordinary predecessor projection excludes mana abilities with activation payments".into());
            }
            GameCommand::CastFaceDown { .. }
            | GameCommand::TurnFaceUp { .. }
            | GameCommand::ActivateWithCreatures { .. }
            | GameCommand::ActivateUntargetedAbility { .. } => {
                return Err(
                    "ordinary predecessor projection excludes untargeted battlefield activation"
                        .into(),
                );
            }
            GameCommand::Ninjutsu { .. }
            | GameCommand::Transmute { .. }
            | GameCommand::Cycle { .. } => {
                return Err("ordinary predecessor projection excludes cycling".into());
            }
            GameCommand::Concede => {
                return Err("ordinary predecessor projection excludes concession inputs".into());
            }
            GameCommand::Pass => CommandView::Pass,
            GameCommand::PlayLand { land } => CommandView::PlayLand {
                land: history.exact(fixture, *land)?,
            },
            GameCommand::ActivateMana { source, ability } => CommandView::ActivateMana {
                source: history.exact(fixture, *source)?,
                ability: crate::mana_ref::view(*ability),
            },
            GameCommand::CastUntargeted { spell, payment: p } => CommandView::CastUntargeted {
                spell: history.exact(fixture, *spell)?,
                payment: p.as_ref().map(payment),
            },
            GameCommand::Cast { spell, target: t } => CommandView::Cast {
                spell: history.exact(fixture, *spell)?,
                target: target(fixture, history, *t)?,
            },
            GameCommand::CastPaid {
                spell,
                target: t,
                payment: p,
            } => CommandView::CastPaid {
                spell: history.exact(fixture, *spell)?,
                target: target(fixture, history, *t)?,
                payment: payment(p),
            },
            GameCommand::ActivateAbility {
                source,
                ability,
                target: t,
                payment: p,
            } => CommandView::ActivateAbility {
                source: history.exact(fixture, *source)?,
                ability: ability.0.to_string(),
                target: target(fixture, history, *t)?,
                payment: p.as_ref().map(payment),
            },
            GameCommand::Reply { decision, choice } => CommandView::Reply {
                decision: decision.0.to_string(),
                choice: reply(fixture, history, choice)?,
            },
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct CardObservation {
    pub object: damage_trace::GameObjectView,
    pub alias: String,
    pub creature: Option<CreatureObservation>,
    pub counters: Option<CounterView>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct CreatureObservation {
    pub power: RulesInteger,
    pub toughness: RulesInteger,
    pub marked_damage: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct CounterView {
    pub plus_one: RulesInteger,
    pub minus_one: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Pile {
    Library { owner: u32 },
    Graveyard { owner: u32 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct OrderedPileAfter {
    pub pile: Pile,
    pub objects_top_to_bottom: Vec<ExactRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct World {
    pub cards: BTreeMap<String, CardObservation>,
    pub tokens: BTreeMap<String, TokenObservation>,
    pub ordered_piles: Vec<OrderedPileAfter>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct TokenObservation {
    pub object: damage_trace::GameObjectView,
    pub program: serde_json::Value,
    pub characteristics: serde_json::Value,
    pub creature: Option<CreatureObservation>,
    pub counters: Option<CounterView>,
}
pub(super) fn world(
    fixture: &Fixture,
    history: &History,
    state: &GameState,
) -> Result<World, String> {
    fixture.require_ordinary_execution()?;
    if state.conclusion().is_some_and(|c| {
        matches!(
            c.cause(),
            spellwake_engine::game::GameConclusionCause::Conceded { .. }
        )
    }) {
        return Err("ordinary predecessor projection excludes conceded roots".into());
    }
    let evaluated = state
        .execution()
        .evaluated_characteristics()
        .map_err(|error| format!("characteristic evaluation failed: {error:?}"))?;
    let mut cards = BTreeMap::new();
    let mut tokens = BTreeMap::new();
    for card in state.execution().window().cards().iter() {
        let creature = evaluated
            .objects
            .get(&card.id())
            // This comparison view includes physical marked damage, so its
            // creature domain remains battlefield-only despite all-zone PT.
            .filter(|value| value.zone == ObjectZone::Battlefield && value.is_creature())
            .and_then(|value| value.power_toughness.as_ref())
            .map(|v| CreatureObservation {
                power: v.derived.power.clone(),
                toughness: v.derived.toughness.clone(),
                marked_damage: state
                    .execution()
                    .battlefield_marked_damage(card.id())
                    .expect("current battlefield creature has physical marks"),
            });
        let counters = state
            .execution()
            .power_toughness_counters(card.id())
            .map(|v| CounterView {
                plus_one: v.plus_one,
                minus_one: v.minus_one,
            });
        if let Some(card_id) = card.card_id() {
            let label = alias(fixture, card_id)?;
            cards.insert(
                card_id.0.to_string(),
                CardObservation {
                    object: damage_trace::game_object_view(card),
                    alias: label,
                    creature,
                    counters,
                },
            );
        } else {
            tokens.insert(
                opaque(card.id()),
                TokenObservation {
                    object: damage_trace::game_object_view(card),
                    program: program::view(
                        state
                            .execution()
                            .base_program(card.id())
                            .ok_or("token baseline missing")?,
                    ),
                    characteristics: projection::current_characteristics(
                        evaluated.get(card.id()).ok_or("token evaluation missing")?,
                    ),
                    creature,
                    counters,
                },
            );
        }
    }
    if cards.len() != fixture.aliases.len() {
        return Err("observed physical inventory is incomplete".into());
    }
    let mut ordered_piles = Vec::new();
    for owner in [A, B] {
        for (pile, ids) in [
            (
                Pile::Library { owner: owner.0 },
                state.execution().window().cards().library(owner),
            ),
            (
                Pile::Graveyard { owner: owner.0 },
                state.execution().window().cards().graveyard(owner),
            ),
        ] {
            ordered_piles.push(OrderedPileAfter {
                pile,
                objects_top_to_bottom: ids
                    .iter()
                    .map(|id| history.required(fixture, *id))
                    .collect::<Result<_, _>>()?,
            });
        }
    }
    Ok(World {
        cards,
        tokens,
        ordered_piles,
    })
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum PriorityView {
    RequiresChecks { candidate: u32 },
    Ready { holder: u32 },
    AllPassed,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum PhaseView {
    Opening,
    TurnWork,
    Priority { phase: PriorityView },
    Resolving { entry: String },
    CheckingTargets { entry: String },
    AllTargetsIllegal { entry: String },
    StepEnded,
}
fn phase(value: WindowPhase) -> PhaseView {
    match value {
        WindowPhase::Opening => PhaseView::Opening,
        WindowPhase::TurnWork => PhaseView::TurnWork,
        WindowPhase::Priority(value) => PhaseView::Priority {
            phase: match value {
                PriorityPhase::RequiresChecks { candidate } => PriorityView::RequiresChecks {
                    candidate: candidate.0,
                },
                PriorityPhase::Ready { holder } => PriorityView::Ready { holder: holder.0 },
                PriorityPhase::AllPassed => PriorityView::AllPassed,
            },
        },
        WindowPhase::Resolving(entry) => PhaseView::Resolving {
            entry: opaque(entry),
        },
        WindowPhase::CheckingTargets(entry) => PhaseView::CheckingTargets {
            entry: opaque(entry),
        },
        WindowPhase::AllTargetsIllegal(entry) => PhaseView::AllTargetsIllegal {
            entry: opaque(entry),
        },
        WindowPhase::StepEnded => PhaseView::StepEnded,
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum WorkView {
    Untap,
    Draw,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage { additional: bool },
    Cleanup,
}
fn work(value: TurnWork) -> WorkView {
    match value {
        TurnWork::Untap => WorkView::Untap,
        TurnWork::Draw => WorkView::Draw,
        TurnWork::DeclareAttackers => WorkView::DeclareAttackers,
        TurnWork::DeclareBlockers => WorkView::DeclareBlockers,
        TurnWork::CombatDamage { additional } => WorkView::CombatDamage { additional },
        TurnWork::Cleanup => WorkView::Cleanup,
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum StageView {
    Priority,
    Work { work: WorkView },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct TurnView {
    pub position: damage_trace::TurnPositionView,
    pub stage: StageView,
    pub lands_played: RulesInteger,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum OutcomeView {
    Winner { seat: u32 },
    Draw,
}
pub(super) fn outcome(value: GameOutcome) -> OutcomeView {
    match value {
        GameOutcome::Winner(seat) => OutcomeView::Winner { seat: seat.0 },
        GameOutcome::Draw => OutcomeView::Draw,
    }
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct OpeningSeatView {
    player: u32,
    kept: bool,
    taken: u8,
    charged: usize,
    can_mulligan: bool,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct OpeningView {
    complete: bool,
    seats: Vec<OpeningSeatView>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct EntropyView {
    position: usize,
    remaining: usize,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct CombatantView {
    object: ExactRef,
    controller: u32,
    control_epoch: String,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct AttackingView {
    combatant: CombatantView,
    defender: u32,
    blocked: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct BlockingView {
    combatant: CombatantView,
    attacker: Option<ExactRef>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct CombatView {
    had_attackers: bool,
    attackers: Vec<AttackingView>,
    blockers: Vec<BlockingView>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct TargetOccurrenceView {
    id: u32,
    clause: u32,
    target: TargetView,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum StackKindView {
    Speed,
    Monarch,
    Emblem {
        id: u64,
    },
    CardSpell {
        card: ExactRef,
    },
    SpellCopy {
        copied_from: String,
        owner: u32,
    },
    ActivatedAbility {
        source: check_trace::CheckGameObjectView,
    },
    TriggeredAbility {
        source: check_trace::CheckGameObjectView,
    },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct StackView {
    entry: String,
    controller: u32,
    kind: StackKindView,
    targets: Vec<TargetOccurrenceView>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum OpeningFrameView {
    Declare {
        player: u32,
        taken: u8,
        can_mulligan: bool,
    },
    Bottom {
        player: u32,
        count: usize,
        exact_hand: Vec<ExactRef>,
    },
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CombatFrameView {
    Attackers {
        revision: String,
        chooser: u32,
        eligible_attackers: Vec<ExactRef>,
        defenders: Vec<u32>,
    },
    Blockers {
        revision: String,
        chooser: u32,
        eligible_blockers: Vec<ExactRef>,
        attackers: Vec<ExactRef>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        restricted_blockers: Vec<RestrictedBlockerView>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        minimum_if_blocked: Vec<BlockMinimumView>,
    },
    DamageAssignments {
        revision: String,
        chooser: u32,
        position: damage_trace::TurnPositionView,
        domains: Vec<AssignmentDomainView>,
    },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct RestrictedBlockerView {
    blocker: ExactRef,
    attackers: Vec<ExactRef>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct BlockMinimumView {
    attacker: ExactRef,
    minimum: usize,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct AssignmentDomainView {
    source: ExactRef,
    power: RulesInteger,
    recipients: Vec<damage_trace::DamageRecipientView>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum FrameView {
    OptionalEffect {
        frame: serde_json::Value,
    },
    ResolutionPayment {
        frame: serde_json::Value,
    },
    CopyTargets {
        frame: serde_json::Value,
    },
    ManaColor {
        frame: serde_json::Value,
    },
    TriggerTarget {
        frame: serde_json::Value,
    },
    TimestampOrder {
        frame: serde_json::Value,
    },
    Opening {
        frame: OpeningFrameView,
    },
    Checks {
        frame: check_trace::CheckDecisionView,
    },
    Triggers {
        frame: check_trace::TriggerOrderView,
    },
    Combat {
        frame: CombatFrameView,
    },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct DecisionView {
    pub id: String,
    pub basis: BasisView,
    pub chooser: u32,
    pub frame: FrameView,
}
fn decision(
    fixture: &Fixture,
    history: &History,
    state: &GameState,
) -> Result<Option<DecisionView>, String> {
    fixture.require_ordinary_execution()?;
    let Some(value) = state.pending_decision() else {
        return Ok(None);
    };
    let refs = |ids: &[ObjectId]| {
        ids.iter()
            .map(|id| history.known(fixture, *id))
            .collect::<Result<Vec<_>, _>>()
    };
    let frame = match value.frame() {
        RuleFrame::PromptChoice(_) => {
            return Err("historical ordinary scopes exclude prompt choices".into());
        }
        RuleFrame::LoyaltyRemoval(_) => {
            return Err("historical ordinary scopes exclude loyalty removal choices".into());
        }
        RuleFrame::ResolutionManaColor(_) => {
            return Err("historical ordinary scopes exclude resolution mana choices".into());
        }
        RuleFrame::CastRecovery(_) => {
            return Err("historical ordinary scopes exclude cast recovery".into());
        }
        RuleFrame::EntryChoice(_)
        | RuleFrame::TokenEntryChoice(_)
        | RuleFrame::TokenAuraChoice(_)
        | RuleFrame::LinkedReturnOrder(_)
        | RuleFrame::LinkedAuraChoice(_) => {
            return Err("historical ordinary scopes exclude pre-entry frames".into());
        }
        RuleFrame::ZoneReplacement(_) => {
            return Err("ordinary observer does not admit zone replacement frames".into());
        }
        RuleFrame::LibraryTop(_) => {
            return Err("historical ordinary scopes exclude library frames".into());
        }
        RuleFrame::ResolutionSelect(_) | RuleFrame::ResolutionGraveyardOrder(_) => {
            return Err(
                "ordinary historical observation scopes exclude resolution-selection frames".into(),
            );
        }
        RuleFrame::OptionalEffect(_) => {
            require_optional_scope(fixture, "frames")?;
            FrameView::OptionalEffect {
                frame: projection::frame(value.frame(), state)?,
            }
        }
        RuleFrame::ResolutionPayment(_) => {
            require_optional_scope(fixture, "frames")?;
            FrameView::ResolutionPayment {
                frame: projection::frame(value.frame(), state)?,
            }
        }
        RuleFrame::CounterRemoval(_) => {
            return Err("counter removal exceeds historical scope".into());
        }
        RuleFrame::Proliferate(_) => return Err("proliferate exceeds historical scope".into()),
        RuleFrame::TriggerTarget(value) => {
            if !matches!(
                fixture.case.observation_contract().observation_scope,
                ObservationScope::Mandatory | ObservationScope::OptionalPayment
            ) {
                return Err(
                    "ordinary predecessor projection excludes trigger target frames".into(),
                );
            }
            FrameView::TriggerTarget {
                frame: projection::trigger_target_view(value),
            }
        }
        RuleFrame::Opening(v) => FrameView::Opening {
            frame: match v {
                OpeningDecision::Declare {
                    player,
                    taken,
                    can_mulligan,
                } => OpeningFrameView::Declare {
                    player: player.0,
                    taken: *taken,
                    can_mulligan: *can_mulligan,
                },
                OpeningDecision::Bottom {
                    player,
                    count,
                    hand,
                } => OpeningFrameView::Bottom {
                    player: player.0,
                    count: *count,
                    exact_hand: refs(hand)?,
                },
                OpeningDecision::PregameBattlefield { .. } => {
                    return Err("historical scope excludes pregame battlefield choices".into());
                }
            },
        },
        RuleFrame::Checks(v) => FrameView::Checks {
            frame: check_trace::check_decision_view(v, state.execution().window().cards(), |id| {
                fixture.aliases[&id].alias.clone()
            })?,
        },
        RuleFrame::Triggers(v) => FrameView::Triggers {
            frame: check_trace::trigger_order_view(v),
        },
        RuleFrame::Combat(v) => FrameView::Combat {
            frame: match v {
                CombatDecision::Attackers(v) => CombatFrameView::Attackers {
                    revision: v.revision().to_string(),
                    chooser: v.chooser().0,
                    eligible_attackers: refs(v.eligible_attackers())?,
                    defenders: v.defenders().iter().map(|v| v.0).collect(),
                },
                CombatDecision::Blockers(v) => CombatFrameView::Blockers {
                    revision: v.revision().to_string(),
                    chooser: v.chooser().0,
                    eligible_blockers: refs(v.eligible_blockers())?,
                    attackers: refs(v.attackers())?,
                    restricted_blockers: v
                        .restricted_blockers()
                        .map(|(id, targets)| {
                            Ok(RestrictedBlockerView {
                                blocker: history.known(fixture, id)?,
                                attackers: refs(targets)?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                    minimum_if_blocked: v
                        .group_minimums()
                        .map(|(id, minimum)| {
                            Ok(BlockMinimumView {
                                attacker: history.known(fixture, id)?,
                                minimum,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                },
                CombatDecision::DamageAssignments(v) => CombatFrameView::DamageAssignments {
                    revision: v.revision().to_string(),
                    chooser: v.chooser().0,
                    position: damage_trace::turn_position_view(v.position()),
                    domains: v
                        .domains()
                        .iter()
                        .map(|v| {
                            Ok(AssignmentDomainView {
                                source: history.known(fixture, v.source())?,
                                power: v.power().clone(),
                                recipients: v
                                    .recipients()
                                    .iter()
                                    .map(|v| damage_trace::damage_recipient_view(*v))
                                    .collect(),
                            })
                        })
                        .collect::<Result<_, String>>()?,
                },
            },
        },
        RuleFrame::CopyTargets(_) => {
            if fixture.case.observation_contract().observation_scope
                != ObservationScope::OptionalPayment
            {
                return Err("unsupported ordinary fixture frame: copy_targets".into());
            }
            FrameView::CopyTargets {
                frame: projection::frame(value.frame(), state)?,
            }
        }
        RuleFrame::Damage(_) => return Err("unsupported ordinary fixture frame: damage".into()),
        RuleFrame::CounterReplacement(_) => {
            return Err("historical scope excludes counter replacement ordering".into());
        }
        RuleFrame::TransformOrder(_) => {
            return Err("historical ordinary profile excludes transforming permanents".into());
        }
        RuleFrame::Mutation(_)
        | RuleFrame::RevealedPiles(_)
        | RuleFrame::PlaneswalkerType(_)
        | RuleFrame::AttackDirection(_)
        | RuleFrame::DrawReplacement(_)
        | RuleFrame::DamageRecipientChoice(_)
        | RuleFrame::DamageSourceChoice(_)
        | RuleFrame::OpponentChoice(_)
        | RuleFrame::ReplacementChooser(_)
        | RuleFrame::MyriadDefender(_)
        | RuleFrame::ReselectAttackDefender(_)
        | RuleFrame::TokenDefender(_) => {
            return Err("historical ordinary fixtures exclude attacking token choices".into());
        }
        RuleFrame::TimestampOrder(_) => {
            if !matches!(
                fixture.case.observation_contract().observation_scope,
                ObservationScope::Mandatory | ObservationScope::OptionalPayment
            ) {
                return Err(
                    "ordinary fixture recipe has no timestamp order decision policy".into(),
                );
            }
            FrameView::TimestampOrder {
                frame: projection::frame(value.frame(), state)?,
            }
        }
        RuleFrame::CreatureType(_) => {
            return Err("ordinary fixture recipe has no creature type decision policy".into());
        }
        RuleFrame::OptionIndex(_) => {
            return Err("ordinary fixture recipe has no option index decision policy".into());
        }
        RuleFrame::UntapKeepTapped(_) => {
            return Err("ordinary fixture recipe has no untap keep-tapped decision policy".into());
        }
        RuleFrame::ManaColor(_) => {
            if fixture.case.observation_contract().observation_scope
                != ObservationScope::OptionalPayment
            {
                return Err("unsupported ordinary fixture frame: mana_color".into());
            }
            FrameView::ManaColor {
                frame: projection::frame(value.frame(), state)?,
            }
        }
    };
    Ok(Some(DecisionView {
        id: value.id().0.to_string(),
        basis: basis(fixture, value.basis())?,
        chooser: value.chooser().0,
        frame,
    }))
}
#[derive(Debug, Clone)]
pub(super) struct Header {
    pub basis: BasisView,
    pub executor_revision: String,
    pub window_revision: String,
    pub registry_revision: String,
    pub participants: Vec<u32>,
    pub active: u32,
    pub combat_rules: &'static str,
    pub turn: Option<TurnView>,
    pub pending_turn_work: Option<WorkView>,
    pub phase: PhaseView,
    pub priority_holder: Option<u32>,
    pub pending: Option<DecisionView>,
    pub instruction_index: Option<usize>,
    pub life: BTreeMap<u32, RulesInteger>,
    pub mana: BTreeMap<u32, BTreeMap<&'static str, RulesInteger>>,
    pub counts_by_owner: BTreeMap<u32, BTreeMap<&'static str, usize>>,
    pub battlefield_counts_by_controller: BTreeMap<u32, usize>,
    pub losses: Vec<u32>,
    pub pending_eliminations: Vec<u32>,
    pub outcome: Option<OutcomeView>,
    pub opening: Option<OpeningView>,
    pub entropy: Option<EntropyView>,
    pub combat: Option<CombatView>,
    epoch: HeaderEpoch,
}
pub(super) fn header(
    fixture: &Fixture,
    history: &History,
    state: &GameState,
) -> Result<Header, String> {
    fixture.require_ordinary_execution()?;
    check_predecessor_semantics(state)?;
    let window = state.execution().window();
    let legacy_stack = || -> Result<Vec<StackView>, String> {
        window
            .entries()
            .iter()
            .map(|entry| {
                let snapshot = |card: &GameObject| {
                    check_trace::check_game_object_view(card, |id| {
                        fixture.aliases[&id].alias.clone()
                    })
                };
                Ok(StackView {
                    entry: opaque(entry.id()),
                    controller: entry.controller().0,
                    kind: match entry.kind() {
                        StackKind::CardSpell(card) => StackKindView::CardSpell {
                            card: history.required(fixture, *card)?,
                        },
                        StackKind::SpellCopy { copied_from, owner } => StackKindView::SpellCopy {
                            copied_from: opaque(copied_from),
                            owner: owner.0,
                        },
                        StackKind::Speed => StackKindView::Speed,
                        StackKind::Monarch => StackKindView::Monarch,
                        StackKind::DelayedCopy { .. } => {
                            return Err(
                                "historical ordinary scopes exclude delayed-copy triggers".into()
                            );
                        }
                        StackKind::Emblem { emblem } => StackKindView::Emblem {
                            id: emblem.sequence(),
                        },
                        StackKind::ActivatedAbility { source } => StackKindView::ActivatedAbility {
                            source: snapshot(source),
                        },
                        StackKind::TriggeredAbility { source } => StackKindView::TriggeredAbility {
                            source: snapshot(source),
                        },
                    },
                    targets: entry
                        .targets()
                        .targets()
                        .iter()
                        .map(|v| {
                            let v = v.choice();
                            Ok(TargetOccurrenceView {
                                id: v.id.0,
                                clause: v.clause.0,
                                target: target(fixture, history, v.target)?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()
    };
    let observe_execution = || -> Result<ExecutionObservation, String> {
        Ok(ExecutionObservation {
            stack: projection::stack_payloads(state.execution()),
            resolution: state
                .execution()
                .resolution_state()
                .map(projection::resolution_state),
            waiting: state
                .execution()
                .waiting_triggers()
                .map(projection::waiting)
                .collect(),
            pending_trigger_placement: state
                .execution()
                .pending_trigger_placement()
                .map_err(|error| format!("pending trigger placement: {error:?}"))?
                .as_ref()
                .map(projection::trigger_placement),
        })
    };
    let epoch = match fixture.case.observation_contract().report_epoch {
        ReportEpoch::Report5 => {
            return Err("selection Report5 header projection is unsupported".into());
        }
        ReportEpoch::Report1 => HeaderEpoch::Legacy(legacy_stack()?),
        ReportEpoch::Report2 => HeaderEpoch::Interaction {
            legacy_stack: legacy_stack()?,
            execution: observe_execution()?,
        },
        ReportEpoch::Report3 => HeaderEpoch::Mandatory(observe_execution()?),
        ReportEpoch::Report4 => HeaderEpoch::OptionalPayment(epoch::OptionalExecutionObservation {
            execution: observe_execution()?,
            pending_work: projection::pending_work(state.execution())?,
            copies: projection::spell_copies(window),
            resolving: projection::resolving_payload(state.execution()),
        }),
    };
    let mut counts = BTreeMap::new();
    let mut controllers = BTreeMap::from([(A.0, 0), (B.0, 0)]);
    for owner in [A, B] {
        counts.insert(
            owner.0,
            BTreeMap::from([
                ("library", 0),
                ("hand", 0),
                ("graveyard", 0),
                ("battlefield", 0),
                ("stack", 0),
                ("exile", 0),
                ("command", 0),
            ]),
        );
    }
    for card in window.cards().iter() {
        let zone = match card.zone() {
            ObjectZone::Library(_) => "library",
            ObjectZone::Hand(_) => "hand",
            ObjectZone::Graveyard(_) => "graveyard",
            ObjectZone::Battlefield => "battlefield",
            ObjectZone::Stack => "stack",
            ObjectZone::Exile => "exile",
            ObjectZone::Command => "command",
        };
        *counts
            .get_mut(&card.owner().0)
            .ok_or("unknown observed owner")?
            .get_mut(zone)
            .ok_or("missing count zone")? += 1;
        if card.zone() == ObjectZone::Battlefield {
            *controllers
                .get_mut(&card.controller().ok_or("battlefield controller absent")?.0)
                .ok_or("unknown controller")? += 1;
        }
    }
    let combatant = |v: spellwake_engine::combat::Combatant| -> Result<CombatantView, String> {
        Ok(CombatantView {
            object: history.known(fixture, v.object())?,
            controller: v.controller().0,
            control_epoch: opaque(v.control_epoch()),
        })
    };
    let combat = state
        .execution()
        .combat()
        .map(|v| {
            Ok::<_, String>(CombatView {
                had_attackers: v.had_attackers(),
                attackers: v
                    .attackers()
                    .map(|v| {
                        Ok(AttackingView {
                            combatant: combatant(v.combatant())?,
                            defender: v.defender().0,
                            blocked: match v.block_status() {
                                BlockStatus::Blocked => "blocked",
                                BlockStatus::Unblocked => "unblocked",
                                BlockStatus::NotDeclared => "not_declared",
                            },
                        })
                    })
                    .collect::<Result<_, String>>()?,
                blockers: v
                    .blockers()
                    .map(|v| {
                        Ok(BlockingView {
                            combatant: combatant(v.combatant())?,
                            attacker: v
                                .attacker()
                                .map(|id| history.known(fixture, id))
                                .transpose()?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
            })
        })
        .transpose()?;
    Ok(Header {
        basis: basis(fixture, state.basis())?,
        executor_revision: state.execution().revision().to_string(),
        window_revision: window.revision().to_string(),
        registry_revision: window.cards().revision().to_string(),
        participants: window.participants().iter().map(|v| v.0).collect(),
        active: window.active_player().0,
        combat_rules: match state.execution().combat_rules() {
            spellwake_engine::combat::CombatRules::UnrestrictedFreeForAll => {
                "unrestricted_free_for_all"
            }
        },
        turn: state.turn().map(|v| TurnView {
            position: damage_trace::turn_position_view(v.position()),
            stage: match v.stage() {
                TurnStage::Priority => StageView::Priority,
                TurnStage::Work(v) => StageView::Work { work: work(v) },
            },
            lands_played: v.lands_played().clone(),
        }),
        pending_turn_work: match state.conclusion().map(|c| c.cause()) {
            Some(spellwake_engine::game::GameConclusionCause::CheckedLosses { .. }) => {
                match state.turn().map(|turn| turn.stage()) {
                    Some(TurnStage::Work(retained)) => Some(work(retained)),
                    Some(TurnStage::Priority) | None => None,
                }
            }
            Some(
                spellwake_engine::game::GameConclusionCause::Conceded { .. }
                | spellwake_engine::game::GameConclusionCause::WonByEffect { .. },
            ) => {
                return Err("ordinary predecessor projection excludes this terminal cause".into());
            }
            None => state.pending_turn_work().map(work),
        },
        phase: phase(window.phase()),
        priority_holder: match window.phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Some(holder.0),
            _ => None,
        },
        pending: decision(fixture, history, state)?,
        instruction_index: state.execution().instruction_index(),
        life: [A, B]
            .iter()
            .map(|p| {
                Ok((
                    p.0,
                    state
                        .execution()
                        .life_total(*p)
                        .ok_or("missing actual life")?
                        .clone(),
                ))
            })
            .collect::<Result<_, String>>()?,
        mana: [A, B]
            .iter()
            .map(|p| {
                (
                    p.0,
                    ManaType::ALL
                        .iter()
                        .map(|t| (mana_type(*t), state.execution().mana(*p, *t)))
                        .collect(),
                )
            })
            .collect(),
        counts_by_owner: counts,
        battlefield_counts_by_controller: controllers,
        losses: state
            .execution()
            .state_based_losses()
            .iter()
            .map(|v| v.0)
            .collect(),
        pending_eliminations: state.pending_eliminations().iter().map(|v| v.0).collect(),
        outcome: state.outcome().map(outcome),
        opening: state.opening().map(|v| OpeningView {
            complete: v.complete(),
            seats: [A, B]
                .iter()
                .map(|p| OpeningSeatView {
                    player: p.0,
                    kept: v.kept(*p),
                    taken: v.mulligans_taken(*p),
                    charged: v.charged(*p),
                    can_mulligan: v.can_mulligan(*p),
                })
                .collect(),
        }),
        entropy: state.entropy().map(|v| EntropyView {
            position: v.position(),
            remaining: v.remaining(),
        }),
        combat,
        epoch,
    })
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ShuffleView {
    player: u32,
    algorithm: &'static str,
    before_exact_order: Vec<ExactRef>,
    after_exact_order: Vec<ExactRef>,
    entropy_start: usize,
    entropy_end: usize,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
// Privileged serialization captures own both actual appearances. Preserve that
// existing layout after adding token origin; no runtime profile currently
// justifies introducing indirection solely to satisfy this size heuristic.
#[allow(clippy::large_enum_variant)]
pub(super) enum DrawView {
    Drawn {
        player: u32,
        before: check_trace::CheckGameObjectView,
        after: check_trace::CheckGameObjectView,
    },
    EmptyLibrary {
        player: u32,
        attempts: RulesInteger,
    },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct TurnTransitionView {
    from: Option<damage_trace::TurnPositionView>,
    to: damage_trace::TurnPositionView,
    active_before: u32,
    active_after: u32,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct MovementView {
    before: check_trace::CheckGameObjectView,
    after: check_trace::CheckGameObjectView,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct LandView {
    player: u32,
    #[serde(flatten)]
    movement: MovementView,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct PermanentView {
    entry: String,
    #[serde(flatten)]
    movement: MovementView,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct ProductionView {
    source: ExactRef,
    ability: serde_json::Value,
    controller: u32,
    produced: BTreeMap<&'static str, RulesInteger>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct PaymentRecordView {
    source: String,
    payer: u32,
    cost: CostView,
    spend: Vec<SpendView>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct ActivationView {
    source: check_trace::CheckGameObjectView,
    source_cost: &'static str,
    resulting_source: ExactRef,
    entry: String,
    ability: String,
    controller: u32,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum DeclarationView {
    Attackers {
        player: u32,
        origin: &'static str,
        attackers: Vec<AttackView>,
        taps: Vec<MovementView>,
    },
    Blockers {
        player: u32,
        origin: &'static str,
        blockers: Vec<BlockView>,
    },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct AssignmentRecordView {
    player: u32,
    origin: &'static str,
    assignments: Vec<AssignmentView>,
}
#[derive(Debug, Clone)]
pub(super) struct Facts {
    pub shuffle_journal: Vec<ShuffleView>,
    pub draw_journal: Vec<DrawView>,
    pub turn_transitions: Vec<TurnTransitionView>,
    pub land_play: Option<LandView>,
    pub mana_production: Option<ProductionView>,
    pub mana_payment: Option<PaymentRecordView>,
    pub ability_activation: Option<ActivationView>,
    pub permanent_entry: Option<PermanentView>,
    pub combat_declarations: Vec<DeclarationView>,
    pub combat_assignments: Vec<AssignmentRecordView>,
    pub damage_journal: damage_trace::DamageJournalView,
    pub check_journal: check_trace::CheckJournalView,
    epoch: JournalEpoch,
}
/// Preserve predecessor exclusions while admitting the bounded physical-spell
/// instruction families represented by the Interaction projection.
fn check_ordinary_projection_scope(
    fixture: &Fixture,
    candidate: &NativeCandidate,
) -> Result<(), String> {
    fixture.require_ordinary_execution()?;
    check_predecessor_semantics(candidate)?;
    let state = candidate.state();
    let execution = state.execution();
    let scope = fixture.case.observation_contract().observation_scope;
    if scope == ObservationScope::Mandatory {
        return epoch::check_mandatory_scope(fixture, candidate);
    }
    if scope == ObservationScope::OptionalPayment {
        return epoch::check_optional_scope(fixture, candidate);
    }
    let interaction = scope == ObservationScope::Interaction;
    if interaction
        && execution
            .window()
            .cards()
            .iter()
            .any(|card| card.card_id().is_none())
    {
        return Err("interaction projection excludes token objects".into());
    }
    if execution.waiting_triggers().next().is_some() {
        return Err("ordinary predecessor projection excludes waiting trigger instances".into());
    }
    if matches!(
        state.pending_decision().map(|pending| pending.frame()),
        Some(RuleFrame::Triggers(_) | RuleFrame::TriggerTarget(_))
    ) {
        return Err("ordinary predecessor projection excludes trigger order frames".into());
    }
    if (execution.resolution_state().is_some() && !interaction)
        || matches!(
            state.pending_decision().map(|pending| pending.frame()),
            Some(RuleFrame::CreatureType(_))
        )
    {
        return Err(
            "ordinary predecessor projection excludes resolution-local values and type frames"
                .into(),
        );
    }
    check_predecessor_rules_journal(candidate.rules_journal())?;
    for entry in execution.window().entries() {
        if interaction {
            if !matches!(entry.kind(), StackKind::CardSpell(_))
                || execution
                    .execution_bindings(entry.id())
                    .is_some_and(|bindings| bindings.trigger.is_some())
            {
                return Err(
                    "interaction projection requires ordinary physical spell entries".into(),
                );
            }
        } else if execution.stack_body(entry.id()).is_some()
            || execution.execution_bindings(entry.id()).is_some()
        {
            return Err(
                "ordinary predecessor projection excludes instruction bodies and execution bindings"
                    .into(),
            );
        }
    }
    if execution
        .resolution_state()
        .is_some_and(|cursor| !cursor.locals().is_empty())
    {
        return Err("interaction projection excludes resolution-local choices".into());
    }
    for (card, program) in execution.programs() {
        if !fixture
            .setup
            .seed
            .cards
            .iter()
            .any(|seed| seed.card == card && &seed.program == program)
        {
            return Err("ordinary predecessor projection requires exact source-guarded fixture Programs including subtypes".into());
        }
        if interaction && !interaction_program(program) {
            return Err(
                "interaction projection excludes unrepresented program capabilities".into(),
            );
        }
    }
    Ok(())
}
/// Admission here bounds this diagnostic projection; the source-qualified
/// Registry remains responsible for the exact finite card identities and traits.
fn interaction_program(program: &spellwake_engine::program::Program) -> bool {
    use spellwake_engine::program::{BodyTarget, CardType, Instruction, ProgramKind};
    if !program.printed_keywords.is_empty()
        || !program.mana.is_empty()
        || program.card_types.len() != 1
        || !program.activated.is_empty()
        || !program.triggered.is_empty()
        || program.continuous.is_some()
        || !program.additional_battlefield_abilities.is_empty()
    {
        return false;
    }
    match &program.kind {
        ProgramKind::Permanent(_) => {
            program.has_type(CardType::Land) || program.has_type(CardType::Creature)
        }
        ProgramKind::Spell(spell)
            if program.has_type(CardType::Instant) || program.has_type(CardType::Sorcery) =>
        {
            match (spell.body.target, spell.body.instructions.as_slice()) {
                (BodyTarget::SingleCreature, [Instruction::ZoneOp(spec)])
                    if matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                        && matches!(
                            spec.selection,
                            spellwake_engine::toolbox::ZoneSelection::All
                        )
                        && matches!(
                            spec.destination,
                            spellwake_engine::toolbox::ZoneDest::Hand
                        )
                        && spec.shuffle_library.is_none() =>
                {
                    true
                }
                (BodyTarget::SingleSpell(filter), [Instruction::CounterSpell])
                    if filter == spellwake_engine::program::SpellFilter::any() =>
                {
                    true
                }
                (BodyTarget::None, [Instruction::DrawCards(spec)])
                    if matches!(
                        spec.players,
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            spellwake_engine::program::ResolvingPlayer::Controller
                        ) | spellwake_engine::toolbox::PlayerSelector::Group(
                            spellwake_engine::program::AffectedPlayers::One(
                                spellwake_engine::program::ResolvingPlayer::Controller
                            )
                        )
                    ) =>
                {
                    matches!(
                        &spec.amount,
                        spellwake_engine::program::ValueExpr::Literal(amount) if *amount == 2.into()
                    )
                }
                _ => false,
            }
        }
        _ => false,
    }
}
fn check_predecessor_rules_journal(journal: &RulesJournal) -> Result<(), String> {
    for record in &journal.records {
        match record {
            RulesRecord::AttackDeclarationReversed { .. }
            | RulesRecord::CastAnnouncementReversed { .. } => {
                return Err(
                    "ordinary predecessor projection excludes reversed cast announcements".into(),
                );
            }
            RulesRecord::SpeedTriggerDiscovered { .. } => {
                return Err("ordinary predecessor projection excludes speed triggers".into());
            }
            RulesRecord::DelayedCopyTriggerDiscovered { .. }
            | RulesRecord::MonarchTriggerDiscovered { .. }
            | RulesRecord::EmblemTriggerDiscovered { .. } => {
                return Err(
                    "ordinary predecessor projection excludes source-independent triggers".into(),
                );
            }
            RulesRecord::GameConcluded { .. } => {
                return Err("ordinary predecessor projection excludes terminal records".into());
            }
            RulesRecord::OptionalEffectOutcome { .. }
            | RulesRecord::ChooserReplaced { .. }
            | RulesRecord::ResolutionPaymentOutcome { .. } => {
                return Err("ordinary Report1–3 exclude resolution choice controls".into());
            }
            RulesRecord::InstructionBranch { .. } | RulesRecord::InstructionJump { .. } => {
                return Err(
                    "ordinary predecessor projection excludes instruction control records".into(),
                );
            }
            RulesRecord::Event(event) => {
                if event.occurrences.iter().any(|occurrence| {
                    matches!(
                        occurrence.value,
                        spellwake_engine::occurrence::RulesOccurrence::CreatureTypeChosen { .. }
                    )
                }) {
                    return Err(
                        "ordinary predecessor projection excludes creature-type choice occurrences"
                            .into(),
                    );
                }
            }
            RulesRecord::TriggerDiscovered(_) => {
                return Err("ordinary predecessor projection excludes trigger captures".into());
            }
            RulesRecord::TriggerPlacementStarted(_)
            | RulesRecord::TriggerTargetSelected { .. }
            | RulesRecord::TriggerRemoved { .. }
            | RulesRecord::TriggerPlaced(_) => {
                return Err("ordinary predecessor projection excludes trigger placements".into());
            }
        }
    }
    Ok(())
}
pub(super) fn facts(
    fixture: &Fixture,
    history: &History,
    source: Option<&GameState>,
    candidate: &NativeCandidate,
) -> Result<Facts, String> {
    fixture.require_ordinary_execution()?;
    check_ordinary_projection_scope(fixture, candidate)?;
    let object = |v: &GameObject| {
        check_trace::check_game_object_view(v, |id| fixture.aliases[&id].alias.clone())
    };
    let movement = |a: &GameObject, b: &GameObject| MovementView {
        before: object(a),
        after: object(b),
    };
    let origin = |v: DeclarationOrigin| match v {
        DeclarationOrigin::PlayerReply => "player_reply",
        DeclarationOrigin::ForcedEmpty => "forced_empty",
    };
    Ok(Facts {
        epoch: match fixture.case.observation_contract().report_epoch {
            ReportEpoch::Report5 => {
                return Err("selection Report5 facts projection is unsupported".into());
            }
            ReportEpoch::Report1 => JournalEpoch::Absent,
            ReportEpoch::Report2 => {
                JournalEpoch::Interaction(projection::journal(candidate.rules_journal()))
            }
            ReportEpoch::Report3 => {
                JournalEpoch::Mandatory(projection::journal(candidate.rules_journal()))
            }
            ReportEpoch::Report4 => {
                JournalEpoch::OptionalPayment(projection::journal(candidate.rules_journal()))
            }
        },
        shuffle_journal: candidate
            .shuffle_journal()
            .iter()
            .map(|v| {
                Ok(ShuffleView {
                    player: v.player.0,
                    algorithm: v.algorithm,
                    before_exact_order: v
                        .before
                        .iter()
                        .map(|id| history.required(fixture, *id))
                        .collect::<Result<_, _>>()?,
                    after_exact_order: v
                        .after
                        .iter()
                        .map(|id| history.required(fixture, *id))
                        .collect::<Result<_, _>>()?,
                    entropy_start: v.entropy_start,
                    entropy_end: v.entropy_end,
                })
            })
            .collect::<Result<_, String>>()?,
        draw_journal: candidate
            .draw_journal()
            .events
            .iter()
            .map(|v| match v {
                DrawEvent::Drawn {
                    player,
                    before,
                    after,
                } => DrawView::Drawn {
                    player: player.0,
                    before: object(before),
                    after: object(after),
                },
                DrawEvent::EmptyLibrary { player, attempts } => DrawView::EmptyLibrary {
                    player: player.0,
                    attempts: attempts.clone(),
                },
            })
            .collect(),
        turn_transitions: candidate
            .turn_transitions()
            .iter()
            .map(|v| TurnTransitionView {
                from: v.from.map(damage_trace::turn_position_view),
                to: damage_trace::turn_position_view(v.to),
                active_before: v.active_before.0,
                active_after: v.active_after.0,
            })
            .collect(),
        land_play: candidate.land_play().map(|v| LandView {
            player: v.player.0,
            movement: movement(&v.before, &v.after),
        }),
        mana_production: candidate
            .mana_production()
            .map(|v| {
                Ok::<_, String>(ProductionView {
                    source: history.known(fixture, v.source)?,
                    ability: crate::mana_ref::view(v.ability),
                    controller: v.controller.0,
                    produced: v
                        .produced
                        .iter()
                        .map(|(t, n)| (mana_type(*t), n.clone()))
                        .collect(),
                })
            })
            .transpose()?,
        mana_payment: candidate.mana_payment().map(|v| PaymentRecordView {
            source: opaque(v.source),
            payer: v.payer.0,
            cost: cost(&v.cost),
            spend: spend(&v.spend),
        }),
        ability_activation: candidate
            .ability_activation()
            .map(|v| {
                Ok::<_, String>(ActivationView {
                    source: object(&v.source),
                    source_cost: {
                        use spellwake_engine::activated::{
                            DiscardCostMode, SelectedSacrificeFilter, SourceCost,
                        };
                        match v.source_cost {
                            SourceCost::TapPermanents { .. }
                            | SourceCost::ExileGraveyard { .. }
                            | SourceCost::PayLife {
                                sacrifice: Some(_), ..
                            }
                            | SourceCost::Discard {
                                mode: DiscardCostMode::SelectedOne,
                                ..
                            } => {
                                return Err(
                                    "historical scope excludes selected activation costs".into()
                                );
                            }
                            SourceCost::SacrificeSelected { filter, .. }
                                if filter != SelectedSacrificeFilter::GrantingPermanent =>
                            {
                                return Err(
                                    "historical scope excludes selected activation costs".into()
                                );
                            }
                            SourceCost::Exile {
                                from_graveyard: true,
                                ..
                            }
                            | SourceCost::GraveyardSelf { unearth: false }
                            | SourceCost::GraveyardDiscard { .. }
                            | SourceCost::GraveyardExileOther { .. }
                            | SourceCost::GraveyardPayLife { .. } => {
                                return Err(
                                    "historical scope excludes graveyard-source activation".into(),
                                );
                            }
                            other => other.trace_label(),
                        }
                    },
                    resulting_source: history.required(fixture, v.resulting_source)?,
                    entry: opaque(v.entry),
                    ability: v.ability.0.to_string(),
                    controller: v.controller.0,
                })
            })
            .transpose()?,
        permanent_entry: candidate.permanent_entry().map(|v| PermanentView {
            entry: opaque(v.entry),
            movement: movement(&v.before, &v.after),
        }),
        combat_declarations: candidate
            .combat_declarations()
            .iter()
            .map(|v| {
                Ok(match v {
                    CombatDeclarationRecord::Attackers {
                        player,
                        origin: o,
                        attackers,
                        taps,
                    } => DeclarationView::Attackers {
                        player: player.0,
                        origin: origin(*o),
                        attackers: attackers
                            .iter()
                            .map(|v| {
                                Ok(AttackView {
                                    attacker: history.known(fixture, v.attacker)?,
                                    defender: v.defender.0,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                        taps: taps.iter().map(|v| movement(&v.before, &v.after)).collect(),
                    },
                    CombatDeclarationRecord::Blockers {
                        player,
                        origin: o,
                        blockers,
                    } => DeclarationView::Blockers {
                        player: player.0,
                        origin: origin(*o),
                        blockers: blockers
                            .iter()
                            .map(|v| {
                                Ok(BlockView {
                                    blocker: history.known(fixture, v.blocker)?,
                                    attacker: history.known(fixture, v.attacker)?,
                                })
                            })
                            .collect::<Result<_, String>>()?,
                    },
                })
            })
            .collect::<Result<_, String>>()?,
        combat_assignments: candidate
            .combat_assignments()
            .iter()
            .map(|v| {
                Ok(AssignmentRecordView {
                    player: v.player.0,
                    origin: match v.origin {
                        AssignmentOrigin::PlayerReply => "player_reply",
                        AssignmentOrigin::Forced => "forced",
                    },
                    assignments: v
                        .assignments
                        .iter()
                        .map(|v| {
                            Ok(AssignmentView {
                                source: history.known(fixture, v.source)?,
                                target: damage_trace::damage_recipient_view(v.target),
                                amount: v.amount.clone(),
                            })
                        })
                        .collect::<Result<_, String>>()?,
                })
            })
            .collect::<Result<_, String>>()?,
        damage_journal: damage_trace::damage_journal_view(candidate.damage_journal()),
        check_journal: check_trace::check_journal_view(
            candidate.check_journal(),
            source
                .unwrap_or_else(|| candidate.state())
                .execution()
                .window()
                .cards(),
            |id| fixture.aliases[&id].alias.clone(),
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellwake_engine::{
        combat::CombatRules,
        effects::SeedCard,
        mana::{ManaAbility, ManaAbilityId, ManaOutput},
        object_identity::ObjectDestination,
        program::Program,
        program::{BodyTarget, ExecutableBody, Instruction, TriggeredAbility, TriggeredAbilityId},
        setup::GameSeed,
        turn::{Step, TurnStart},
    };

    fn tapped_trigger_candidate(count: usize) -> (Fixture, NativeCandidate) {
        tapped_candidate_with_target(count, BodyTarget::None)
    }
    fn tapped_candidate_with_target(
        count: usize,
        target: BodyTarget,
    ) -> (Fixture, NativeCandidate) {
        let fixture = Fixture::new(
            super::super::fixture::CaseId::G01,
            crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit"),
        )
        .unwrap();
        let mut program = Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::White, 1.into())])),
            }],
        );
        for index in 0..count {
            program.triggered.push(TriggeredAbility {
                grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
                id: TriggeredAbilityId(index as u64),
                condition: spellwake_engine::program::TriggerSpec::becomes_tapped(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                body: ExecutableBody::new(
                    target,
                    vec![match target {
                        BodyTarget::None => Instruction::DrawCards(Box::new(
                            spellwake_engine::toolbox::DrawCardsSpec::simple(
                                spellwake_engine::toolbox::PlayerSelector::Resolving(
                                    spellwake_engine::program::ResolvingPlayer::Controller,
                                ),
                                spellwake_engine::program::ValueExpr::Literal(1.into()),
                            ),
                        )),
                        BodyTarget::SingleOpponent | BodyTarget::SinglePlayer => {
                            Instruction::AdjustTargetLife {
                                adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
                            }
                        }
                        BodyTarget::SingleCreature => Instruction::Damage { amount: 1.into() },
                        _ => panic!("unsupported test target family"),
                    }],
                ),
            });
        }
        let state = GameState::initialize_scheduled(
            fixture.game,
            fixture.context.clone(),
            GameSeed::with_uniform_life(
                vec![A, B],
                A,
                vec![SeedCard {
                    card: CardId(1),
                    owner: A,
                    destination: ObjectDestination::Battlefield { controller: A },
                    program,
                }],
                vec![CardId(1)],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                fixture.setup.seed.creature_vocabulary.clone(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            TurnStart {
                number: 2,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let source = state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        let candidate = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: GameCommand::ActivateMana {
                    source,
                    ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
                },
            })
            .unwrap();
        (fixture, candidate)
    }

    #[test]
    fn ordinary_observer_rejects_new_values_after_the_body_has_completed() {
        use spellwake_engine::program::CreatureTypeSlot;
        let fixture = Fixture::new(
            super::super::fixture::CaseId::G01,
            crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit"),
        )
        .unwrap();
        let vocabulary = fixture.setup.seed.creature_vocabulary.clone();
        let seed = GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![SeedCard {
                card: CardId(99999),
                owner: A,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost {
                        generic: 0.into(),
                        required: BTreeMap::new(),
                    },
                    ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ChooseCreatureType {
                            into: CreatureTypeSlot::new(0),
                            excluding: Default::default(),
                        }],
                    ),
                ),
            }],
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        );
        let mut state = GameState::initialize(fixture.game, fixture.context.clone(), seed)
            .unwrap()
            .into_state();
        let spell = state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(99999))
            .unwrap();
        let advance = |state: &GameState, actor, command| {
            state
                .advance(GameInput {
                    basis: state.basis().clone(),
                    actor,
                    command,
                })
                .unwrap()
        };
        state = advance(
            &state,
            A,
            GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        )
        .into_state();
        state = advance(&state, A, GameCommand::Pass).into_state();
        let pending = advance(&state, B, GameCommand::Pass);
        assert_eq!(
            check_ordinary_projection_scope(&fixture, &pending).unwrap_err(),
            "ordinary predecessor projection excludes resolution-local values and type frames"
        );
        state = pending.into_state();
        let complete = advance(
            &state,
            A,
            GameCommand::Reply {
                decision: state.pending_decision().unwrap().id(),
                choice: DecisionReply::CreatureType(vocabulary.resolve_label("Elf").unwrap()),
            },
        );
        assert!(complete.state().execution().resolution_state().is_none());
        assert!(complete.state().execution().window().entries().is_empty());
        assert_eq!(
            check_ordinary_projection_scope(&fixture, &complete).unwrap_err(),
            "ordinary predecessor projection excludes creature-type choice occurrences"
        );
    }

    #[test]
    fn ordinary_observer_admits_only_the_fixture_source_qualified_cat_programs() {
        let fixture = Fixture::new(
            super::super::fixture::CaseId::G01,
            crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit"),
        )
        .unwrap();
        let initial = GameState::initialize(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.seed.clone(),
        )
        .unwrap();
        check_ordinary_projection_scope(&fixture, &initial).unwrap();
        let mut seed = fixture.setup.seed.clone();
        let creature = seed
            .cards
            .iter_mut()
            .find(|card| !card.program.creature_subtypes.is_empty())
            .unwrap();
        creature.program.creature_subtypes.clear();
        let altered = GameState::initialize(fixture.game, fixture.context.clone(), seed).unwrap();
        assert_eq!(
            check_ordinary_projection_scope(&fixture, &altered).unwrap_err(),
            "ordinary predecessor projection requires exact source-guarded fixture Programs including subtypes"
        );
    }

    #[test]
    fn ordinary_observer_rejects_actual_waiting_captures_and_trigger_frame() {
        let (fixture, candidate) = tapped_trigger_candidate(2);
        assert_eq!(candidate.state().execution().waiting_triggers().count(), 2);
        assert!(matches!(
            candidate.state().pending_decision().unwrap().frame(),
            RuleFrame::Triggers(_)
        ));
        assert_eq!(
            facts(&fixture, &History::default(), None, &candidate).unwrap_err(),
            "ordinary predecessor projection excludes waiting trigger instances"
        );
    }

    #[test]
    fn ordinary_observer_rejects_actual_capture_and_placement_records() {
        let (_, candidate) = tapped_trigger_candidate(1);
        for (select, expected) in [
            (
                0,
                "ordinary predecessor projection excludes trigger captures",
            ),
            (
                1,
                "ordinary predecessor projection excludes trigger placements",
            ),
        ] {
            let journal = RulesJournal {
                records: candidate
                    .rules_journal()
                    .records
                    .iter()
                    .filter(|record| {
                        matches!(
                            (select, record),
                            (0, RulesRecord::TriggerDiscovered(_))
                                | (1, RulesRecord::TriggerPlaced(_))
                        )
                    })
                    .cloned()
                    .collect(),
            };
            assert!(!journal.records.is_empty());
            assert_eq!(
                check_predecessor_rules_journal(&journal).unwrap_err(),
                expected
            );
        }
    }

    #[test]
    fn ordinary_observer_rejects_retained_body_and_binding_without_a_current_trigger_append() {
        let (fixture, placed) = tapped_trigger_candidate(1);
        let state = placed.into_state();
        let candidate = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: GameCommand::Pass,
            })
            .unwrap();
        assert!(candidate.rules_journal().records.is_empty());
        assert_eq!(candidate.state().execution().waiting_triggers().count(), 0);
        let entry = candidate.state().execution().window().entries()[0].id();
        assert!(candidate.state().execution().stack_body(entry).is_some());
        assert!(
            candidate
                .state()
                .execution()
                .execution_bindings(entry)
                .unwrap()
                .trigger
                .is_some()
        );
        assert_eq!(
            facts(&fixture, &History::default(), Some(&state), &candidate).unwrap_err(),
            "ordinary predecessor projection excludes instruction bodies and execution bindings"
        );
    }

    #[test]
    fn ordinary_predecessor_rejects_actual_target_frame_reply_and_each_new_placement_record() {
        let (fixture, pending) = tapped_candidate_with_target(1, BodyTarget::SinglePlayer);
        assert!(matches!(
            pending.state().pending_decision().unwrap().frame(),
            RuleFrame::TriggerTarget(_)
        ));
        assert_eq!(
            decision(&fixture, &History::default(), pending.state()).unwrap_err(),
            "ordinary predecessor projection excludes trigger target frames"
        );
        let choice = DecisionReply::TriggerTarget(spellwake_engine::targets::TargetRef::Player(A));
        assert_eq!(
            reply(&fixture, &History::default(), &choice).unwrap_err(),
            "ordinary predecessor projection excludes trigger target replies"
        );
        let accepted = pending
            .state()
            .advance(GameInput {
                basis: pending.state().basis().clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision: pending.state().pending_decision().unwrap().id(),
                    choice,
                },
            })
            .unwrap();
        let (_, removed) = tapped_candidate_with_target(1, BodyTarget::SingleCreature);
        for (candidate, kind) in [(&pending, 0), (&accepted, 1), (&removed, 2)] {
            let records = candidate
                .rules_journal()
                .records
                .iter()
                .filter(|record| {
                    matches!(
                        (kind, record),
                        (0, RulesRecord::TriggerPlacementStarted(_))
                            | (1, RulesRecord::TriggerTargetSelected { .. })
                            | (2, RulesRecord::TriggerRemoved { .. })
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(records.len(), 1);
            assert_eq!(
                check_predecessor_rules_journal(&RulesJournal { records }).unwrap_err(),
                "ordinary predecessor projection excludes trigger placements"
            );
        }
    }
}
