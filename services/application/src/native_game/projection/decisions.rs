//! Actual frame-to-description mapping. Private native values never become wire IDs.
use super::*;
use spellwake_engine::{
    checks::CheckDecision,
    combat::CombatDecision,
    damage::{DamageEffectId, DamageRecipient},
    effects::{
        CopyTargetChoice, DamageDecision, EffectError, LibraryActionKind, LibraryChoiceKind,
        OptionalEffectChoice, PendingWorkKind, ZoneOptionalAnswer, ZoneReplacementRequest,
    },
    game::{DecisionReply, GameCommand, GameError, GameInput, RuleFrame},
    opening::{MulliganChoice, OpeningDecision},
    targets::TargetRef,
};
impl Projection<'_, '_> {
    fn immutable_object_label(&self, id: ObjectId) -> Result<String, NativeProjectionError> {
        let source = self.scope.source;
        let object = source
            .execution()
            .window()
            .cards()
            .get(id)
            .ok_or(NativeProjectionError::MissingNativeValue)?;
        let definition = source
            .execution()
            .copiable_program(id)
            .ok_or(NativeProjectionError::MissingNativeValue)?;
        Ok(self
            .display
            .object(object, definition)?
            .characteristics
            .name)
    }
    fn library_label(&self, id: ObjectId) -> Result<String, NativeProjectionError> {
        let source = self.scope.source;
        let position = source
            .execution()
            .window()
            .cards()
            .library(self.scope.binding.actor)
            .iter()
            .position(|object| *object == id)
            .ok_or(NativeProjectionError::HiddenReference)?;
        match source.execution().pending_library_kind() {
            // Mill grants no look. Do not consult identity/display data here.
            Some(LibraryActionKind::Mill) => Ok(format!("Library card {}", position + 1)),
            Some(LibraryActionKind::Scry | LibraryActionKind::Surveil) => {
                self.immutable_object_label(id)
            }
            None => Err(NativeProjectionError::HiddenReference),
        }
    }
    fn replacement_object_label(&mut self, id: ObjectId) -> Result<String, NativeProjectionError> {
        if self.scope.source.execution().pending_work_kind() == Some(PendingWorkKind::LibraryTop) {
            return self.library_label(id);
        }
        let handle = self.visible_object(id)?;
        self.object_label(&handle)
    }
    fn library_order(&mut self, ids: &[ObjectId]) -> Result<PlayerDomain, NativeProjectionError> {
        let mut labels = vec![];
        for id in ids {
            let label = self.library_label(*id)?;
            labels.push(self.option(NativeReference::LibraryCard(*id), label)?);
        }
        Ok(PlayerDomain::Ordering {
            objects: labels.iter().map(|entry| entry.handle.clone()).collect(),
            labels,
        })
    }
    fn option(
        &mut self,
        reference: NativeReference,
        label: String,
    ) -> Result<PlayerOption, NativeProjectionError> {
        Ok(PlayerOption {
            handle: self.issue(reference)?,
            label,
        })
    }
    fn objects_domain(
        &mut self,
        ids: &[ObjectId],
        required: usize,
    ) -> Result<PlayerDomain, NativeProjectionError> {
        let objects = ids
            .iter()
            .map(|id| self.visible_object(*id))
            .collect::<Result<_, _>>()?;
        Ok(PlayerDomain::Objects {
            objects,
            minimum: count(required)?,
            maximum: count(required)?,
        })
    }
    fn object_order(&mut self, ids: &[ObjectId]) -> Result<PlayerDomain, NativeProjectionError> {
        let objects = ids
            .iter()
            .map(|id| self.visible_object(*id))
            .collect::<Result<Vec<_>, _>>()?;
        let labels = objects
            .iter()
            .map(|handle| {
                Ok(PlayerOption {
                    handle: handle.clone(),
                    label: self.object_label(handle)?,
                })
            })
            .collect::<Result<_, NativeProjectionError>>()?;
        Ok(PlayerDomain::Ordering { objects, labels })
    }
    fn recipient(&mut self, value: DamageRecipient) -> Result<PlayerTarget, NativeProjectionError> {
        self.target(match value {
            DamageRecipient::Creature(id) => TargetRef::Object(id),
            DamageRecipient::Player(seat) => TargetRef::Player(seat),
        })
    }
    fn target_label(&mut self, target: TargetRef) -> Result<String, NativeProjectionError> {
        let value = self.target(target)?;
        match value {
            PlayerTarget::Object { handle } => self.object_label(&handle),
            PlayerTarget::Stack { handle } => self
                .stack
                .iter()
                .find(|entry| entry.handle == handle)
                .map(|entry| entry.label.clone())
                .ok_or(NativeProjectionError::MissingNativeValue),
            PlayerTarget::Seat { seat_id } => self
                .scope
                .binding
                .seats
                .iter()
                .position(|entry| entry.seat_id == seat_id)
                .map(|index| format!("Player {}", index + 1))
                .ok_or(NativeProjectionError::MissingNativeValue),
        }
    }
    fn copy_probe(&self, choice: CopyTargetChoice) -> Result<bool, NativeProjectionError> {
        let source = self.scope.source;
        let pending = source
            .pending_decision()
            .ok_or(NativeProjectionError::MissingNativeValue)?;
        match source.advance(GameInput {
            basis: source.basis().clone(),
            actor: self.scope.binding.actor,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::CopyTargets(choice),
            },
        }) {
            Ok(_) => Ok(true),
            Err(GameError::Effect(EffectError::IllegalTarget)) => Ok(false),
            Err(cause) => Err(NativeBridgeError::Native(Box::new(cause)).into()),
        }
    }
    pub(super) fn decisions(&mut self) -> Result<Vec<PlayerDecision>, NativeProjectionError> {
        let source = self.scope.source;
        crate::native_game::clocks::supported_wait(source)?;
        let Some(pending) = source.pending_decision() else {
            return Ok(vec![]);
        };
        if pending.chooser() != self.scope.binding.actor {
            return Ok(vec![]);
        }
        let (kind, domain) = match pending.frame() {
            RuleFrame::ZoneReplacement(frame)
                if crate::native_game::handles::supports_zone_replacement(source) =>
            {
                let mut options = vec![];
                match frame.request() {
                    ZoneReplacementRequest::Select { applications } => {
                        for application in applications {
                            if !self.visible.contains(&application.effect.source) {
                                return Err(NativeProjectionError::HiddenReference);
                            }
                            let source_label =
                                self.immutable_object_label(application.effect.source)?;
                            let card_label =
                                self.replacement_object_label(application.event.before())?;
                            let definition = self
                                .evaluated
                                .get(application.effect.source)
                                .and_then(|current| {
                                    current.printed_text.zone_replacements.iter().find(
                                        |definition| definition.id == application.effect.ability,
                                    )
                                })
                                .ok_or(NativeProjectionError::MissingNativeValue)?;
                            let destination = |value| match value {
                                spellwake_engine::program::PublicZoneDestination::Graveyard => {
                                    "graveyard"
                                }
                                spellwake_engine::program::PublicZoneDestination::Exile => "exile",
                            };
                            let label = format!(
                                "{source_label}: put {card_label} into {} instead of {}",
                                destination(definition.replace_with),
                                destination(definition.destination)
                            );
                            options.push(
                                self.option(NativeReference::ZoneApplication(*application), label)?,
                            );
                        }
                    }
                    ZoneReplacementRequest::ConfirmOptional { .. } => {
                        for (answer, label) in [
                            (ZoneOptionalAnswer::Apply, "Apply replacement"),
                            (ZoneOptionalAnswer::Decline, "Decline replacement"),
                        ] {
                            options.push(
                                self.option(NativeReference::ZoneOptional(answer), label.into())?,
                            );
                        }
                    }
                }
                (
                    DecisionKind::ZoneReplacement,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::ZoneReplacement(_) => return Err(NativeBridgeError::Unsupported.into()),
            RuleFrame::Checks(CheckDecision::CleanupSelect(_)) => {
                return Err(NativeBridgeError::Unsupported.into());
            }
            RuleFrame::LibraryTop(frame) => (
                match frame.kind() {
                    LibraryChoiceKind::Scry => DecisionKind::Scry,
                    LibraryChoiceKind::Surveil => DecisionKind::Surveil,
                },
                self.library_order(frame.domain_top_first())?,
            ),
            RuleFrame::ResolutionSelect(frame) => (
                DecisionKind::SelectResolution,
                self.objects_domain(frame.domain(), frame.required())?,
            ),
            RuleFrame::ResolutionGraveyardOrder(frame) => {
                if source.execution().pending_work_kind() == Some(PendingWorkKind::LibraryTop) {
                    (
                        DecisionKind::OrderLibraryGraveyard,
                        self.library_order(frame.domain())?,
                    )
                } else {
                    (
                        DecisionKind::OrderResolutionGraveyard,
                        self.object_order(frame.domain())?,
                    )
                }
            }
            RuleFrame::ReplacementChooser(frame) => {
                let options = frame
                    .candidates()
                    .iter()
                    .map(|player| {
                        let label = format!("Player {}", self.app_seat(*player)?);
                        self.option(NativeReference::ReplacementChooser(*player), label)
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::ReplacementChooser,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::OptionalEffect(frame) => {
                let options = frame
                    .options()
                    .iter()
                    .map(|choice| {
                        self.option(
                            NativeReference::OptionalEffect(*choice),
                            match choice {
                                OptionalEffectChoice::Take => "Take effect",
                                OptionalEffectChoice::Decline => "Decline effect",
                            }
                            .into(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::OptionalEffect,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::ResolutionPayment(frame) => {
                let cost = frame.cost();
                let minimum_spend = cost
                    .required
                    .iter()
                    .map(|(kind, amount)| {
                        Ok(ManaSpend {
                            color: mana(*kind),
                            amount: quantity(amount)?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                // Allocation bounds quote the exact cost, not current affordability.
                // A fresh pool is projected separately; native Pay owns that check.
                let maximum_spend = ManaType::ALL
                    .into_iter()
                    .map(|kind| {
                        Ok(ManaSpend {
                            color: mana(kind),
                            amount: quantity(&cost.total())?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::ResolutionPayment,
                    PlayerDomain::Payment {
                        minimum_spend,
                        maximum_spend,
                        total: quantity(&cost.total())?,
                        allow_decline: true,
                    },
                )
            }
            RuleFrame::TimestampOrder(frame) => {
                let mut objects = vec![];
                let mut labels = vec![];
                for preview in frame.members() {
                    let label = self.display.token_preview(preview)?;
                    let handle = self.issue(NativeReference::TokenMember(preview.member))?;
                    objects.push(handle.clone());
                    labels.push(PlayerOption { handle, label });
                }
                (
                    DecisionKind::TimestampOrder,
                    PlayerDomain::Ordering { objects, labels },
                )
            }
            RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) => (
                DecisionKind::Discard,
                self.objects_domain(frame.domain(), frame.required())?,
            ),
            RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) => (
                DecisionKind::OrderGraveyard,
                self.object_order(frame.domain())?,
            ),
            RuleFrame::Checks(CheckDecision::Legend(frame)) => {
                let conflicts = frame
                    .conflicts()
                    .iter()
                    .map(|conflict| {
                        Ok(LegendDomain {
                            conflict: self.issue(NativeReference::Legend(conflict.id()))?,
                            objects: conflict
                                .domain()
                                .iter()
                                .map(|id| self.visible_object(*id))
                                .collect::<Result<_, _>>()?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (DecisionKind::KeepLegend, PlayerDomain::Legend { conflicts })
            }
            RuleFrame::Triggers(frame) => {
                let mut objects = vec![];
                let mut labels = vec![];
                for id in frame.instances() {
                    let instance = source
                        .execution()
                        .waiting_triggers()
                        .find(|value| value.id == *id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let label = self.display.trigger(instance)?;
                    let handle = self.issue(NativeReference::Trigger(*id))?;
                    objects.push(handle.clone());
                    labels.push(PlayerOption { handle, label });
                }
                (
                    DecisionKind::OrderTriggers,
                    PlayerDomain::Ordering { objects, labels },
                )
            }
            RuleFrame::TriggerTarget(frame) => {
                let targets = frame
                    .domain()
                    .iter()
                    .map(|target| self.target(*target))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::TriggerTarget,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::Combat(CombatDecision::Attackers(frame)) => {
                let defenders = frame
                    .defenders()
                    .iter()
                    .map(|seat| {
                        Ok(PlayerTarget::Seat {
                            seat_id: self.app_seat(*seat)?,
                        })
                    })
                    .collect::<Result<Vec<_>, NativeProjectionError>>()?;
                let choices = frame
                    .eligible_attackers()
                    .iter()
                    .map(|id| {
                        Ok(AttackerDomain {
                            attacker: self.visible_object(*id)?,
                            defenders: defenders.clone(),
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::DeclareAttackers,
                    PlayerDomain::Attackers {
                        choices,
                        required: vec![],
                    },
                )
            }
            RuleFrame::Combat(CombatDecision::Blockers(frame)) => {
                let attackers = frame
                    .attackers()
                    .iter()
                    .map(|id| self.visible_object(*id))
                    .collect::<Result<Vec<_>, _>>()?;
                let choices = frame
                    .eligible_blockers()
                    .iter()
                    .map(|id| {
                        Ok(BlockerDomain {
                            blocker: self.visible_object(*id)?,
                            attackers: attackers.clone(),
                            minimum: count(0)?,
                            maximum: count(1)?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::DeclareBlockers,
                    PlayerDomain::Blockers {
                        choices,
                        required: vec![],
                    },
                )
            }
            RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) => {
                let sources = frame
                    .domains()
                    .iter()
                    .map(|domain| {
                        Ok(CombatDamageDomain {
                            source: self.visible_object(domain.source())?,
                            targets: domain
                                .recipients()
                                .iter()
                                .map(|target| self.recipient(*target))
                                .collect::<Result<_, _>>()?,
                            total: quantity(domain.power())?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::AssignCombatDamage,
                    PlayerDomain::CombatDamage { sources },
                )
            }
            RuleFrame::Opening(OpeningDecision::Declare { can_mulligan, .. }) => {
                let mut options = vec![self.option(
                    NativeReference::Mulligan(MulliganChoice::Keep),
                    "Keep".into(),
                )?];
                if *can_mulligan {
                    options.push(self.option(
                        NativeReference::Mulligan(MulliganChoice::Mulligan),
                        "Mulligan".into(),
                    )?);
                }
                (
                    DecisionKind::Mulligan,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::Opening(OpeningDecision::Bottom {
                count: required,
                hand,
                ..
            }) => (DecisionKind::Bottom, self.objects_domain(hand, *required)?),
            RuleFrame::CopyTargets(_) => {
                let mut options = vec![];
                if self.copy_probe(CopyTargetChoice::Retain)? {
                    options.push(self.option(
                        NativeReference::CopyTargets(CopyTargetChoice::Retain),
                        "Keep original targets".into(),
                    )?);
                }
                let targets: Vec<_> = self
                    .visible
                    .iter()
                    .copied()
                    .map(TargetRef::Object)
                    .chain(
                        self.scope
                            .binding
                            .seats
                            .iter()
                            .map(|entry| TargetRef::Player(entry.native_seat)),
                    )
                    .chain(
                        source
                            .execution()
                            .window()
                            .entries()
                            .iter()
                            .map(|entry| TargetRef::Stack(entry.id())),
                    )
                    .collect();
                for target in targets {
                    let choice = CopyTargetChoice::Change(target);
                    if self.copy_probe(choice)? {
                        let label = format!("Change target to {}", self.target_label(target)?);
                        options.push(self.option(NativeReference::CopyTargets(choice), label)?);
                    }
                }
                if options.is_empty() {
                    return Err(NativeProjectionError::MissingNativeValue);
                }
                (
                    DecisionKind::CopyTargets,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::Damage(DamageDecision::Order(frame)) => {
                let mut options = vec![];
                for application in frame.applications() {
                    let label = match application.effect {
                        DamageEffectId::Prevention(id) => {
                            let effect = source
                                .execution()
                                .prevention_shield(id)
                                .ok_or(NativeProjectionError::MissingNativeValue)?;
                            format!(
                                "Prevent up to {} damage",
                                String::from(effect.remaining.clone())
                            )
                        }
                        DamageEffectId::Multiplier(id) => {
                            let effect = source
                                .execution()
                                .damage_multiplier(id)
                                .ok_or(NativeProjectionError::MissingNativeValue)?;
                            format!("Multiply damage by {}", String::from(effect.factor.clone()))
                        }
                    };
                    options.push(
                        self.option(NativeReference::DamageApplication(*application), label)?,
                    );
                }
                (
                    DecisionKind::DamageOrder,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::Damage(DamageDecision::Allocation(frame)) => {
                let attempts = frame
                    .eligible()
                    .iter()
                    .map(|attempt| {
                        ensure_public_damage_source(attempt.source(), self.scope.binding.actor)?;
                        Ok(PreventionDomain {
                            attempt: self
                                .issue(NativeReference::DamageAttempt(attempt.attempt()))?,
                            maximum: quantity(attempt.amount())?,
                            label: self.display.prevention_attempt(attempt)?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::AllocatePrevention,
                    PlayerDomain::Prevention {
                        attempts,
                        required: quantity(frame.required())?,
                    },
                )
            }
            RuleFrame::ManaColor(frame) => {
                let options = frame
                    .colors()
                    .iter()
                    .map(|kind| {
                        self.option(
                            NativeReference::ManaColor(*kind),
                            mana_label(*kind).to_owned(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::ManaColor,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::CreatureType(frame) => {
                let vocabulary = source.execution().creature_vocabulary();
                if vocabulary.id() != frame.vocabulary() {
                    return Err(NativeProjectionError::MissingNativeValue);
                }
                let mut options = vec![];
                for label in vocabulary.members() {
                    let id = vocabulary
                        .resolve_label(label)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    if !frame.excluding().contains(&id) {
                        options
                            .push(self.option(NativeReference::CreatureType(id), label.clone())?);
                    }
                }
                (
                    DecisionKind::CreatureType,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
        };
        Ok(vec![PlayerDecision {
            decision: self.issue(NativeReference::Decision(pending.id()))?,
            basis: self.scope.basis().clone(),
            responding_seat_ids: vec![self.scope.viewer_seat_id()],
            visibility: DecisionVisibility::RespondingSeat,
            kind,
            domain,
            deadline_at: self.metadata.deadline_at.clone(),
        }])
    }
}
