//! Finite context-bound input policy. Every pass and choice has a script entry.
use super::fixture::{A, B, CaseId, Fixture, PLAINS_MANA_ABILITY, other, range, seat_name};
use spellwake_engine::{
    checks::{CheckBoundary, CheckDecision},
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision},
    draw::DrawEvent,
    game::{
        CandidateBase, DecisionId, DecisionReply, GameBasis, GameCommand, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    mana::{ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectId},
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackKind, WindowPhase},
    turn::{Step, TurnPosition, TurnStage, TurnWork},
};
use std::collections::{BTreeMap, BTreeSet};

/// One actual announcement/completion relation shared by finite catalogue strategies.
pub(super) struct AcceptedCast {
    pub(super) entry: spellwake_engine::stack_window::StackEntryId,
    pub(super) object: ObjectId,
}
/// Bind a named source to one completed native cast and its real incarnation change.
pub(super) fn accepted_cast(
    fixture: &Fixture,
    input: &GameInput,
    candidate: &NativeCandidate,
    alias: &str,
) -> Result<AcceptedCast, String> {
    use spellwake_engine::occurrence::{EventCause, RulesOccurrence, RulesRecord};
    let announced = match input.command {
        GameCommand::CastUntargeted { spell, .. }
        | GameCommand::CastPaid { spell, .. }
        | GameCommand::Cast { spell, .. } => spell,
        _ => return Err("cast intent is not an actual cast command".into()),
    };
    let mut completed = vec![];
    for record in &candidate.rules_journal().records {
        if let RulesRecord::Event(event) = record {
            for occurrence in &event.occurrences {
                if let RulesOccurrence::CastCompleted(cast) = &occurrence.value {
                    if !matches!(event.cause,EventCause::CastCompleted{entry} if entry==cast.entry())
                    {
                        return Err("cast completion event/entry mismatch".into());
                    }
                    completed.push(cast.as_ref());
                }
            }
        }
    }
    let [cast] = completed.as_slice() else {
        return Err("cast must produce exactly one actual completed-cast occurrence".into());
    };
    if cast.controller() != input.actor
        || cast.spell().object.card_id() != Some(fixture.card(alias)?)
    {
        return Err("actual cast receipt differs from named source/controller".into());
    }
    let mut movements = 0;
    for record in &candidate.rules_journal().records {
        if let RulesRecord::Event(event) = record
            && matches!(event.cause,EventCause::CastAnnouncement{entry} if entry==cast.entry())
        {
            for occurrence in &event.occurrences {
                if let RulesOccurrence::ZoneChange(change) = &occurrence.value
                    && change.before.object.id() == announced
                    && change.after.object.id() == cast.spell().object.id()
                {
                    movements += 1;
                }
            }
        }
    }
    if movements != 1 {
        return Err("cast announcement does not join its actual source incarnation once".into());
    }
    Ok(AcceptedCast {
        entry: cast.entry(),
        object: cast.spell().object.id(),
    })
}

/// Successful finite prepared boundary; serialization stays with the shared recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StopKind {
    PreparedReadyPriority {
        boundary_ref: &'static str,
        position: TurnPosition,
        active: SeatId,
        holder: SeatId,
    },
}

#[derive(Debug, Clone)]
enum Recipe {
    Declare {
        taken: u8,
        choice: MulliganChoice,
    },
    Bottom {
        domain: Vec<String>,
        selected: Vec<String>,
    },
    Pass {
        stack_spell: Option<String>,
    },
    Land(String),
    Tap(String),
    Cast(String),
    Attackers {
        domain: Vec<String>,
        selected: Vec<String>,
    },
    Blockers {
        domain: Vec<String>,
        attackers: Vec<String>,
        selected: Vec<(String, String)>,
    },
    Discard {
        domain: Vec<String>,
        selected: String,
    },
    Order {
        domain: Vec<String>,
        selected: Vec<String>,
    },
}
#[derive(Debug, Clone)]
struct ScriptEntry {
    id: String,
    turn: Option<(u64, Step)>,
    active: SeatId,
    actor: SeatId,
    recipe: Recipe,
}
/// The one selected finite strategy. Native rules state remains in GameState.
#[derive(Debug, Clone)]
enum Strategy {
    Historical(Vec<ScriptEntry>),
    Teaching(Box<super::teaching::TeachingPolicy>),
    Interaction(Box<super::interaction::InteractionPolicy>),
    Mandatory(Box<super::mandatory::MandatoryPolicy>),
    OptionalPayment(Box<super::mandatory::FinitePolicy>),
    Selection(Box<super::mandatory::FinitePolicy>),
}
#[derive(Debug, Clone)]
enum PreparedIntent {
    Historical,
    Teaching(super::teaching::Intent),
    Interaction(super::interaction::Intent),
    Mandatory(Box<super::mandatory::MandatoryIntent>),
    OptionalPayment(Box<super::mandatory::FiniteIntent>),
    Selection(Box<super::mandatory::FiniteIntent>),
}
/// Exactly one derived input/name/intent at a reached boundary. The input owns its sole basis.
#[derive(Debug, Clone)]
pub(super) struct PreparedInput {
    pub(super) input: GameInput,
    pub(super) policy_entry_ref: String,
    intent: PreparedIntent,
}
impl PreparedInput {
    /// Exact matched selected annotation evidence. This is privileged, not seat-safe,
    /// not a serialized Report5 and not proof of native state installation.
    pub(in crate::ordinary_game) fn selection_metadata(
        &self,
    ) -> Option<super::selection::probes::PreparedSelectionMetadata<'_>> {
        match &self.intent {
            PreparedIntent::Selection(intent) => intent.selection_metadata(),
            _ => None,
        }
    }
    pub(super) fn recipe_probes(&self) -> &[super::probes::RejectionProbe] {
        match &self.intent {
            PreparedIntent::Mandatory(intent) => intent.probes(),
            PreparedIntent::OptionalPayment(intent) => intent.probes(),
            PreparedIntent::Selection(intent) => intent.probes(),
            _ => &[],
        }
    }
}
/// Common authority/history plus one selected strategy and at most one issued input.
#[derive(Debug, Clone)]
pub struct Policy {
    fixture: Fixture,
    strategy: Strategy,
    cursor: usize,
    issued: Option<PreparedInput>,
    pub(crate) previous_basis: Option<GameBasis>,
    pub(crate) previous_decision: Option<DecisionId>,
    pub(crate) old_library_objects: BTreeMap<CardId, ObjectId>,
}
/// An exclusively borrowed, validated policy update. Dropping it leaves the
/// policy unchanged; committing it performs only the prepared field updates.
#[must_use = "commit policy acceptance only with the actual installed game state"]
pub(super) struct PolicyAcceptance<'a> {
    policy: &'a mut Policy,
    strategy: Strategy,
    previous_basis: GameBasis,
    previous_decision: Option<DecisionId>,
    drawn: Vec<(CardId, ObjectId)>,
    cursor: usize,
}
impl PolicyAcceptance<'_> {
    pub(super) fn commit(self) {
        self.policy.strategy = self.strategy;
        self.policy.previous_basis = Some(self.previous_basis);
        self.policy.previous_decision = self.previous_decision;
        self.policy.old_library_objects.extend(self.drawn);
        self.policy.cursor = self.cursor;
        self.policy.issued = None;
    }
}
impl Policy {
    pub fn new(fixture: &Fixture) -> Result<Self, String> {
        let strategy = match fixture.case {
            CaseId::Selection(case) => {
                Strategy::Selection(Box::new(super::mandatory::FinitePolicy::from_selection(
                    fixture,
                    super::selection::recipes::recipe(case)?,
                )?))
            }
            super::fixture::CaseId::Teaching(_) => Strategy::Teaching(Box::new(
                super::teaching::TeachingPolicy::new(fixture)?
                    .ok_or("teaching strategy missing")?,
            )),
            super::fixture::CaseId::Interaction(_) => Strategy::Interaction(Box::new(
                super::interaction::InteractionPolicy::new(fixture)?
                    .ok_or("interaction strategy missing")?,
            )),
            super::fixture::CaseId::Mandatory(_) => {
                Strategy::Mandatory(Box::new(super::mandatory::MandatoryPolicy::new(fixture)?))
            }
            super::fixture::CaseId::Optional(case) => {
                Strategy::OptionalPayment(Box::new(super::mandatory::FinitePolicy::from_optional(
                    fixture,
                    super::optional::recipe(case)?,
                )?))
            }
            _ => {
                let entries = script(fixture.case)?;
                if entries.iter().map(|e| &e.id).collect::<BTreeSet<_>>().len() != entries.len() {
                    return Err("duplicate ordinary script identity".into());
                }
                Strategy::Historical(entries)
            }
        };
        Ok(Self {
            fixture: fixture.clone(),
            strategy,
            cursor: 0,
            issued: None,
            previous_basis: None,
            previous_decision: None,
            old_library_objects: BTreeMap::new(),
        })
    }
    /// Bind qualification authority to the actual installed initializer and the
    /// complete genuine source-owned Fixture. This is not durable host acceptance.
    /// Auxiliary native recomputation and a required foreign prefix are returned
    /// separately from the main accepted stream; a failed bind leaves Policy unchanged.
    /// # Errors
    /// Rejects a nonselected/already-issued policy, double binding, foreign source,
    /// same-basis different full seed/result/state, or unavailable helper authority.
    pub fn bind_selection_initializer(
        &mut self,
        source: &super::selection::SelectionSource,
        installed: &GameState,
        initializer: &NativeCandidate,
    ) -> Result<super::selection::SelectionBindingEvidence, super::selection::SelectionBindingError>
    {
        if self.issued.is_some() {
            return Err(super::selection::SelectionBindingError::caller(
                "initializer binding cannot replace an issued policy input",
            ));
        }
        match &mut self.strategy {
            Strategy::Selection(p) => p.bind_selection_initializer(source, installed, initializer),
            _ => Err(super::selection::SelectionBindingError::caller(
                "initializer binding requires selected finite policy",
            )),
        }
    }
    #[cfg(test)]
    pub(in crate::ordinary_game) fn selected_finite_for_test(
        &mut self,
    ) -> Option<&mut super::mandatory::FinitePolicy> {
        match &mut self.strategy {
            Strategy::Selection(p) => Some(p),
            _ => None,
        }
    }
    /// Accepted selected annotation evidence only; privileged and not seat-safe.
    pub(in crate::ordinary_game) fn selection_observation(
        &self,
    ) -> Option<super::selection::authority::SelectionProgressObservation> {
        match &self.strategy {
            Strategy::Selection(p) => Some(p.selection_observation()),
            _ => None,
        }
    }
    pub(super) fn required_recipe_probe_ids(&self) -> Vec<String> {
        match &self.strategy {
            Strategy::Mandatory(p) => p.required_probe_ids(),
            Strategy::OptionalPayment(p) => p.required_probe_ids(),
            Strategy::Selection(p) => p.required_probe_ids(),
            _ => vec![],
        }
    }
    pub(super) fn completion_probes(
        &self,
        state: &GameState,
    ) -> Result<Vec<super::probes::RejectionProbe>, String> {
        match &self.strategy {
            Strategy::Mandatory(p) => p.completion_probes(state),
            Strategy::OptionalPayment(p) => p.completion_probes(state),
            Strategy::Selection(p) => p.completion_probes(state),
            _ => Ok(vec![]),
        }
    }
    pub(super) fn interaction(&self) -> Option<&super::interaction::InteractionPolicy> {
        match &self.strategy {
            Strategy::Interaction(p) => Some(p),
            _ => None,
        }
    }
    pub(super) fn mandatory(&self) -> Option<&super::mandatory::MandatoryPolicy> {
        match &self.strategy {
            Strategy::Mandatory(p) => Some(p),
            _ => None,
        }
    }
    pub(super) fn reached_probe_limit(&self) -> Option<u64> {
        match &self.strategy {
            Strategy::OptionalPayment(p) | Strategy::Selection(p) => p.reached_probe_limit(),
            _ => None,
        }
    }
    pub fn consumed(&self) -> usize {
        self.cursor
    }
    pub fn script_len(&self) -> usize {
        match &self.strategy {
            Strategy::Historical(entries) => entries.len(),
            _ => self.cursor,
        }
    }
    /// Prepare once without consuming an intent or adopting any native candidate.
    /// None is reserved for an actual terminal or validated bounded completion.
    pub(super) fn prepare(&self, state: &GameState) -> Result<Option<PreparedInput>, String> {
        if self.issued.is_some() {
            return Err("policy already has an issued input".into());
        }
        if state.outcome().is_some() || self.completed_boundary(state)?.is_some() {
            return Ok(None);
        }
        let (input, policy_entry_ref, intent) = match &self.strategy {
            Strategy::Historical(entries) => {
                let entry = entries
                    .get(self.cursor)
                    .ok_or("ordinary script exhausted before outcome")?;
                self.bind_context(entry, state)
                    .map_err(|e| format!("{}: {e}", entry.id))?;
                (
                    GameInput {
                        basis: state.basis().clone(),
                        actor: entry.actor,
                        command: self.command(entry, state)?,
                    },
                    entry.id.clone(),
                    PreparedIntent::Historical,
                )
            }
            Strategy::Teaching(p) => {
                let (input, intent) = p.prepare(state)?;
                (
                    input,
                    "teaching.actual-input".into(),
                    PreparedIntent::Teaching(intent),
                )
            }
            Strategy::Interaction(p) => {
                let (input, intent) = p.prepare(state)?;
                let name = p.entry_id(state, &intent);
                (input, name, PreparedIntent::Interaction(intent))
            }
            Strategy::Mandatory(p) => {
                let (input, name, intent) = p.prepare(state)?;
                (input, name, PreparedIntent::Mandatory(Box::new(intent)))
            }
            Strategy::OptionalPayment(p) => {
                let (input, name, intent) = p.prepare(state)?;
                (
                    input,
                    name,
                    PreparedIntent::OptionalPayment(Box::new(intent)),
                )
            }
            Strategy::Selection(p) => {
                let (input, name, intent) = p.prepare(state)?;
                (input, name, PreparedIntent::Selection(Box::new(intent)))
            }
        };
        Ok(Some(PreparedInput {
            input,
            policy_entry_ref,
            intent,
        }))
    }
    /// Issue the owned prepared input only at its exact current authority boundary.
    pub(super) fn issue(
        &mut self,
        state: &GameState,
        prepared: PreparedInput,
    ) -> Result<GameInput, String> {
        if self.issued.is_some()
            || prepared.input.basis != *state.basis()
            || state.outcome().is_some()
        {
            return Err("prepared input is stale, terminal or already issued".into());
        }
        if let Some(decision) = state.pending_decision() {
            let matching_reply = matches!(&prepared.input.command,GameCommand::Reply{decision:id,..} if *id==decision.id());
            let payment_mana = matches!(&self.strategy, Strategy::OptionalPayment(_))
                && matches!(&prepared.intent, PreparedIntent::OptionalPayment(intent) if intent.permits_payment_funding())
                && matches!(decision.frame(), RuleFrame::ResolutionPayment(frame) if frame.payer() == prepared.input.actor)
                && matches!(prepared.input.command, GameCommand::ActivateMana { .. });
            if prepared.input.actor != decision.chooser() || !(matching_reply || payment_mana) {
                return Err("prepared input differs from issued decision authority".into());
            }
        } else {
            if matches!(prepared.input.command, GameCommand::Reply { .. }) {
                return Err("prepared reply has no actual decision".into());
            }
            if state.execution().window().phase()
                != WindowPhase::Priority(PriorityPhase::Ready {
                    holder: prepared.input.actor,
                })
            {
                return Err("prepared actor does not hold actual ready priority".into());
            }
        }
        if !matches!(
            (&self.strategy, &prepared.intent),
            (Strategy::Historical(_), PreparedIntent::Historical)
                | (Strategy::Teaching(_), PreparedIntent::Teaching(_))
                | (Strategy::Interaction(_), PreparedIntent::Interaction(_))
                | (Strategy::Mandatory(_), PreparedIntent::Mandatory(_))
                | (
                    Strategy::OptionalPayment(_),
                    PreparedIntent::OptionalPayment(_)
                )
                | (Strategy::Selection(_), PreparedIntent::Selection(_))
        ) {
            return Err("prepared intent strategy mismatch".into());
        }
        let input = prepared.input.clone();
        self.issued = Some(prepared);
        Ok(input)
    }
    pub fn accept(&mut self, input: &GameInput, candidate: &NativeCandidate) -> Result<(), String> {
        self.prepare_accept(input, candidate)?.commit();
        Ok(())
    }
    /// Validate the same issued input and strategy receipts without advancing
    /// policy history before the caller installs the returned native candidate.
    pub(super) fn prepare_accept(
        &mut self,
        input: &GameInput,
        candidate: &NativeCandidate,
    ) -> Result<PolicyAcceptance<'_>, String> {
        let prepared = self
            .issued
            .as_ref()
            .ok_or("policy accepted without issued prepared input")?;
        if prepared.input != *input {
            return Err("accepted input differs from the exact issued policy input".into());
        }
        if candidate.base() != &CandidateBase::Advance(input.basis.clone()) {
            return Err("candidate does not descend from issued input basis".into());
        }
        let next = candidate.state().basis();
        if next.game != input.basis.game
            || next.context != input.basis.context
            || next.epoch != input.basis.epoch
            || Some(next.revision) != input.basis.revision.checked_add(1)
        {
            return Err("candidate successor basis differs from one accepted root input".into());
        }
        let mut strategy = self.strategy.clone();
        match (&mut strategy, prepared.intent.clone()) {
            (Strategy::Historical(_), PreparedIntent::Historical) => {}
            (Strategy::Teaching(p), PreparedIntent::Teaching(intent)) => p.accept(intent)?,
            (Strategy::Interaction(p), PreparedIntent::Interaction(intent)) => {
                p.accept(input, candidate, intent)?
            }
            (Strategy::Mandatory(p), PreparedIntent::Mandatory(intent)) => {
                p.accept(input, candidate, *intent)?
            }
            (Strategy::OptionalPayment(p), PreparedIntent::OptionalPayment(intent)) => {
                p.accept(input, candidate, *intent)?
            }
            (Strategy::Selection(p), PreparedIntent::Selection(intent)) => {
                p.accept(input, candidate, *intent)?
            }
            _ => return Err("issued strategy changed before accept".into()),
        }
        let mut drawn = vec![];
        for event in &candidate.draw_journal().events {
            if let DrawEvent::Drawn { before, .. } = event {
                drawn.push((
                    before.card_id().ok_or("draw receipt is not physical")?,
                    before.id(),
                ));
            }
        }
        let previous_decision = match input.command {
            GameCommand::Reply { decision, .. } => Some(decision),
            _ => self.previous_decision,
        };
        let cursor = self
            .cursor
            .checked_add(1)
            .ok_or("policy acceptance cursor exhausted")?;
        Ok(PolicyAcceptance {
            previous_basis: input.basis.clone(),
            previous_decision,
            strategy,
            drawn,
            cursor,
            policy: self,
        })
    }
    pub(super) fn completed_boundary(&self, state: &GameState) -> Result<Option<StopKind>, String> {
        if self.issued.is_some() {
            return Err("input remains issued at completion boundary".into());
        }
        match &self.strategy {
            Strategy::Interaction(p) => p.completed_boundary(state),
            Strategy::Mandatory(p) => p.completed_boundary(state),
            Strategy::OptionalPayment(p) => p.completed_boundary(state),
            Strategy::Selection(p) => p.completed_boundary(state),
            _ => Ok(None),
        }
    }
    pub fn finish(&self, state: &GameState) -> Result<(), String> {
        if self.issued.is_some() {
            return Err("input remains issued at finish".into());
        }
        match &self.strategy {
            Strategy::Teaching(p) => return p.finish(state),
            Strategy::Interaction(p) => return p.finish(state),
            Strategy::Mandatory(p) => return p.finish(state),
            Strategy::OptionalPayment(p) => return p.finish(state),
            Strategy::Selection(p) => return p.finish(state),
            Strategy::Historical(entries) => {
                if self.cursor != entries.len() {
                    return Err("outcome left unconsumed script entries".into());
                }
            }
        }
        if state.outcome().is_none() {
            return Err("script ended without a natural outcome".into());
        }
        if state.basis().game != self.fixture.game || state.basis().context != self.fixture.context
        {
            return Err("terminal game/context changed".into());
        }
        if state.entropy().is_none_or(|t| t.remaining() != 0) {
            return Err("unused or absent authored entropy at terminal".into());
        }
        Ok(())
    }
    fn bind_context(&self, entry: &ScriptEntry, state: &GameState) -> Result<(), String> {
        let window = state.execution().window();
        if state.basis().game != self.fixture.game
            || state.basis().context != self.fixture.context
            || state.basis().epoch != 0
        {
            return Err("unexpected fixture game/context/epoch".into());
        }
        if window.participants() != [A, B] || window.active_player() != entry.active {
            return Err("unexpected ordered participants or active player".into());
        }
        match (entry.turn, state.turn()) {
            (None, None) if window.phase() == WindowPhase::Opening => {}
            (Some((number, step)), Some(turn))
                if turn.position().number == number
                    && turn.position().step == step
                    && turn.position().cleanup_iteration == 0
                    && !turn.position().additional_damage => {}
            _ => return Err("unexpected opening/turn/step boundary".into()),
        }
        if let Some(pending) = state.pending_decision()
            && (pending.basis() != state.basis()
                || pending.chooser() != entry.actor
                || pending.frame().chooser() != entry.actor)
        {
            return Err("pending frame basis/chooser does not match entry".into());
        }
        Ok(())
    }
    fn object(&self, state: &GameState, alias: &str) -> Result<ObjectId, String> {
        self.fixture.current_object(state, alias)
    }
    fn objects(&self, state: &GameState, aliases: &[String]) -> Result<Vec<ObjectId>, String> {
        aliases
            .iter()
            .map(|alias| self.object(state, alias))
            .collect()
    }
    fn domain(
        &self,
        state: &GameState,
        actual: &[ObjectId],
        aliases: &[String],
    ) -> Result<(), String> {
        let expected = self.objects(state, aliases)?;
        if actual.len() != expected.len()
            || actual.iter().copied().collect::<BTreeSet<_>>().len() != actual.len()
            || actual.iter().copied().collect::<BTreeSet<_>>() != expected.into_iter().collect()
        {
            return Err(
                "current exact decision domain differs from the complete script domain".into(),
            );
        }
        Ok(())
    }
    fn ready(
        &self,
        entry: &ScriptEntry,
        state: &GameState,
        stack_spell: Option<&str>,
    ) -> Result<(), String> {
        let window = state.execution().window();
        if state.pending_decision().is_some()
            || state.pending_turn_work().is_some()
            || state
                .turn()
                .is_none_or(|turn| turn.stage() != TurnStage::Priority)
            || window.phase()
                != WindowPhase::Priority(PriorityPhase::Ready {
                    holder: entry.actor,
                })
        {
            return Err("script action/pass does not have its actual Ready holder".into());
        }
        match (stack_spell, window.entries()) {
            (None, []) => Ok(()),
            (Some(alias), [entry])
                if entry.controller() == window.active_player()
                    && entry.kind() == &StackKind::CardSpell(self.object(state, alias)?) =>
            {
                Ok(())
            }
            _ => Err("script stack condition mismatch".into()),
        }
    }
    fn work(&self, state: &GameState, work: TurnWork) -> Result<(), String> {
        if state.pending_turn_work() != Some(work)
            || state.execution().window().phase() != WindowPhase::TurnWork
        {
            return Err("pending frame is outside its exact mandatory turn work".into());
        }
        Ok(())
    }
    fn command(&self, entry: &ScriptEntry, state: &GameState) -> Result<GameCommand, String> {
        match &entry.recipe {
            Recipe::Pass { stack_spell } => {
                self.ready(entry, state, stack_spell.as_deref())?;
                return Ok(GameCommand::Pass);
            }
            Recipe::Land(alias) => {
                self.ready(entry, state, None)?;
                return Ok(GameCommand::PlayLand {
                    land: self.object(state, alias)?,
                });
            }
            Recipe::Tap(alias) => {
                self.ready(entry, state, None)?;
                return Ok(GameCommand::ActivateMana {
                    source: self.object(state, alias)?,
                    ability: PLAINS_MANA_ABILITY,
                });
            }
            Recipe::Cast(alias) => {
                self.ready(entry, state, None)?;
                return Ok(GameCommand::CastUntargeted {
                    spell: self.object(state, alias)?,
                    payment: Some(lion_payment()),
                });
            }
            _ => {}
        }
        if !state.execution().window().entries().is_empty() {
            return Err("unexpected stack during scripted decision".into());
        }
        let pending = state
            .pending_decision()
            .ok_or("scripted decision is absent")?;
        let choice = match (&entry.recipe, pending.frame()) {
            (
                Recipe::Declare { taken, choice },
                RuleFrame::Opening(OpeningDecision::Declare {
                    player,
                    taken: actual,
                    can_mulligan,
                }),
            ) if *player == entry.actor && actual == taken && *can_mulligan => {
                DecisionReply::Mulligan(*choice)
            }
            (
                Recipe::Bottom { domain, selected },
                RuleFrame::Opening(OpeningDecision::Bottom {
                    player,
                    count,
                    hand,
                }),
            ) if *player == entry.actor && *count == selected.len() => {
                self.domain(state, hand, domain)?;
                DecisionReply::Bottom(self.objects(state, selected)?)
            }
            (
                Recipe::Attackers { domain, selected },
                RuleFrame::Combat(CombatDecision::Attackers(frame)),
            ) => {
                self.work(state, TurnWork::DeclareAttackers)?;
                if frame.revision() != state.execution().revision()
                    || frame.defenders() != [other(entry.actor)?]
                {
                    return Err("attacker frame revision/defender mismatch".into());
                }
                self.domain(state, frame.eligible_attackers(), domain)?;
                DecisionReply::DeclareAttackers(
                    self.objects(state, selected)?
                        .into_iter()
                        .map(|attacker| AttackDeclaration {
                            attacker,
                            defender: other(entry.actor).expect("actor checked"),
                            defending_planeswalker: None,
                        })
                        .collect(),
                )
            }
            (
                Recipe::Blockers {
                    domain,
                    attackers,
                    selected,
                },
                RuleFrame::Combat(CombatDecision::Blockers(frame)),
            ) => {
                self.work(state, TurnWork::DeclareBlockers)?;
                if frame.revision() != state.execution().revision() {
                    return Err("blocker frame revision mismatch".into());
                }
                self.domain(state, frame.eligible_blockers(), domain)?;
                self.domain(state, frame.attackers(), attackers)?;
                DecisionReply::DeclareBlockers(
                    selected
                        .iter()
                        .map(|(blocker, attacker)| {
                            Ok(BlockDeclaration {
                                blocker: self.object(state, blocker)?,
                                attacker: self.object(state, attacker)?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                )
            }
            (
                Recipe::Discard { domain, selected },
                RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)),
            ) => {
                self.work(state, TurnWork::Cleanup)?;
                let position = state.turn().ok_or("cleanup without turn")?.position();
                if frame.revision() != state.execution().revision()
                    || frame.boundary() != CheckBoundary::Cleanup(position)
                    || frame.required() != 1
                {
                    return Err("cleanup revision/boundary/cardinality mismatch".into());
                }
                self.domain(state, frame.domain(), domain)?;
                DecisionReply::Discard(vec![self.object(state, selected)?])
            }
            (
                Recipe::Order { domain, selected },
                RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)),
            ) => {
                if frame.revision() != state.execution().revision()
                    || frame.boundary() != CheckBoundary::Priority
                    || state
                        .turn()
                        .is_none_or(|turn| turn.stage() != TurnStage::Priority)
                    || state.execution().window().phase()
                        != WindowPhase::Priority(PriorityPhase::RequiresChecks {
                            candidate: entry.active,
                        })
                {
                    return Err("graveyard order revision/priority boundary mismatch".into());
                }
                self.domain(state, frame.domain(), domain)?;
                DecisionReply::OrderGraveyard(self.objects(state, selected)?)
            }
            _ => return Err("unknown or mismatched scripted frame/reply family".into()),
        };
        Ok(GameCommand::Reply {
            decision: pending.id(),
            choice,
        })
    }
}
pub(crate) fn lion_payment() -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: ManaType::White,
            amount: 2.into(),
        }],
    }
}

fn script(case: CaseId) -> Result<Vec<ScriptEntry>, String> {
    let mut entries = vec![];
    let opening = [
        (
            A,
            0,
            if case.is_trade() {
                MulliganChoice::Mulligan
            } else {
                MulliganChoice::Keep
            },
        ),
        (B, 0, MulliganChoice::Keep),
    ];
    for (actor, taken, choice) in opening {
        entries.push(ScriptEntry {
            id: format!(
                "{}.opening.{}.declare.{taken}",
                case.as_str(),
                seat_name(actor)?
            ),
            turn: None,
            active: A,
            actor,
            recipe: Recipe::Declare { taken, choice },
        });
    }
    if case.is_trade() {
        let mut domain = range("A", "L", 1, 2);
        domain.extend(range("A", "P", 1, 5));
        entries.push(ScriptEntry {
            id: format!("{}.opening.A.bottom.1", case.as_str()),
            turn: None,
            active: A,
            actor: A,
            recipe: Recipe::Bottom {
                domain,
                selected: vec!["A.P5".into()],
            },
        });
        entries.push(ScriptEntry {
            id: format!("{}.opening.A.declare.1", case.as_str()),
            turn: None,
            active: A,
            actor: A,
            recipe: Recipe::Declare {
                taken: 1,
                choice: MulliganChoice::Keep,
            },
        });
    }
    let last: u64 = match case {
        CaseId::G01 => 23,
        CaseId::G02a | CaseId::G02b => 29,
        CaseId::G03 => 108,
        CaseId::Teaching(_)
        | CaseId::Interaction(_)
        | CaseId::Mandatory(_)
        | CaseId::Optional(_)
        | CaseId::Selection(_) => {
            return Err("catalogue cases use their explicit ordinary policy".into());
        }
    };
    for turn in 1..=last {
        let active = if turn % 2 == 1 { A } else { B };
        let n = turn.div_ceil(2);
        rounds(
            &mut entries,
            case,
            turn,
            active,
            Step::Upkeep,
            None,
            "ordinary",
        )?;
        if case == CaseId::G03 && turn == last {
            break;
        }
        if turn != 1 {
            rounds(
                &mut entries,
                case,
                turn,
                active,
                Step::Draw,
                None,
                "ordinary",
            )?;
        }
        actions(&mut entries, case, turn, active, n, Step::PrecombatMain)?;
        rounds(
            &mut entries,
            case,
            turn,
            active,
            Step::PrecombatMain,
            None,
            "ordinary",
        )?;
        rounds(
            &mut entries,
            case,
            turn,
            active,
            Step::BeginningCombat,
            None,
            "ordinary",
        )?;
        let (domain, selected) = attack_recipe(case, active, n);
        let attacking = !selected.is_empty();
        if !domain.is_empty() {
            push(
                &mut entries,
                case,
                ScriptPosition {
                    turn,
                    active,
                    step: Step::DeclareAttackers,
                },
                active,
                "attackers",
                Recipe::Attackers { domain, selected },
            );
        }
        rounds(
            &mut entries,
            case,
            turn,
            active,
            Step::DeclareAttackers,
            None,
            "ordinary",
        )?;
        if attacking {
            if case.is_trade() && active == A && n == 5 {
                push(
                    &mut entries,
                    case,
                    ScriptPosition {
                        turn,
                        active: A,
                        step: Step::DeclareBlockers,
                    },
                    B,
                    "blockers",
                    Recipe::Blockers {
                        domain: range("B", "L", 1, 2),
                        attackers: range("A", "L", 1, 2),
                        selected: vec![
                            ("B.L1".into(), "A.L1".into()),
                            ("B.L2".into(), "A.L2".into()),
                        ],
                    },
                );
            }
            rounds(
                &mut entries,
                case,
                turn,
                active,
                Step::DeclareBlockers,
                None,
                "ordinary",
            )?;
            if turn == last {
                break;
            }
            if case.is_trade() && active == A && n == 5 {
                for actor in [A, B] {
                    let domain = range(seat_name(actor)?, "L", 1, 2);
                    let mut selected = domain.clone();
                    if case == CaseId::G02a && actor == B || case == CaseId::G02b && actor == A {
                        selected.reverse();
                    }
                    push(
                        &mut entries,
                        case,
                        ScriptPosition {
                            turn,
                            active: A,
                            step: Step::CombatDamage,
                        },
                        actor,
                        "graveyard_order",
                        Recipe::Order { domain, selected },
                    );
                }
            }
            rounds(
                &mut entries,
                case,
                turn,
                active,
                Step::CombatDamage,
                None,
                "ordinary",
            )?;
        }
        rounds(
            &mut entries,
            case,
            turn,
            active,
            Step::EndCombat,
            None,
            "ordinary",
        )?;
        actions(&mut entries, case, turn, active, n, Step::PostcombatMain)?;
        rounds(
            &mut entries,
            case,
            turn,
            active,
            Step::PostcombatMain,
            None,
            "ordinary",
        )?;
        rounds(
            &mut entries,
            case,
            turn,
            active,
            Step::End,
            None,
            "ordinary",
        )?;
        let discard = match case {
            CaseId::G01 if active == B => Some(format!("B.P{}", n + 7)),
            CaseId::G02a | CaseId::G02b if active == B && n == 1 => Some("B.P6".into()),
            CaseId::G03 if turn != 1 => Some(format!(
                "{}.P{}",
                seat_name(active)?,
                if active == A { n + 6 } else { n + 7 }
            )),
            _ => None,
        };
        if let Some(selected) = discard {
            let mut domain = if case.is_trade() {
                let mut hand = range("B", "L", 1, 2);
                hand.extend(range("B", "P", 1, 5));
                hand
            } else {
                range(seat_name(active)?, "P", 1, 7)
            };
            domain.push(selected.clone());
            push(
                &mut entries,
                case,
                ScriptPosition {
                    turn,
                    active,
                    step: Step::Cleanup,
                },
                active,
                "discard",
                Recipe::Discard { domain, selected },
            );
        }
    }
    Ok(entries)
}
fn step_name(step: Step) -> &'static str {
    match step {
        Step::Untap => "untap",
        Step::Upkeep => "upkeep",
        Step::Draw => "draw",
        Step::PrecombatMain => "precombat_main",
        Step::BeginningCombat => "beginning_combat",
        Step::DeclareAttackers => "declare_attackers",
        Step::DeclareBlockers => "declare_blockers",
        Step::CombatDamage => "combat_damage",
        Step::EndCombat => "end_combat",
        Step::PostcombatMain => "postcombat_main",
        Step::End => "end",
        Step::Cleanup => "cleanup",
    }
}
#[derive(Clone, Copy)]
struct ScriptPosition {
    turn: u64,
    active: SeatId,
    step: Step,
}
fn push(
    entries: &mut Vec<ScriptEntry>,
    case: CaseId,
    position: ScriptPosition,
    actor: SeatId,
    label: &str,
    recipe: Recipe,
) {
    let ScriptPosition { turn, active, step } = position;
    entries.push(ScriptEntry {
        id: format!(
            "{}.{turn}.{}.{}.{}",
            case.as_str(),
            step_name(step),
            actor.0,
            label
        ),
        turn: Some((turn, step)),
        active,
        actor,
        recipe,
    });
}
fn rounds(
    entries: &mut Vec<ScriptEntry>,
    case: CaseId,
    turn: u64,
    active: SeatId,
    step: Step,
    spell: Option<String>,
    label: &str,
) -> Result<(), String> {
    for actor in [active, other(active)?] {
        push(
            entries,
            case,
            ScriptPosition { turn, active, step },
            actor,
            label,
            Recipe::Pass {
                stack_spell: spell.clone(),
            },
        );
    }
    Ok(())
}
fn actions(
    entries: &mut Vec<ScriptEntry>,
    case: CaseId,
    turn: u64,
    active: SeatId,
    n: u64,
    step: Step,
) -> Result<(), String> {
    let land = if step == Step::PrecombatMain {
        match case {
            CaseId::G01 if active == A => Some(n),
            CaseId::G02a | CaseId::G02b if active == A => Some(if n <= 4 { n } else { n + 1 }),
            CaseId::G02a | CaseId::G02b if active == B && n >= 2 => Some(match n {
                2 => 1,
                3 => 2,
                4 => 3,
                5 => 4,
                6 => 5,
                _ => n,
            }),
            _ => None,
        }
    } else {
        None
    };
    if let Some(index) = land {
        let alias = format!("{}.P{index}", seat_name(active)?);
        push(
            entries,
            case,
            ScriptPosition { turn, active, step },
            active,
            "play_land",
            Recipe::Land(alias),
        );
    }
    let lion = match case {
        CaseId::G01 if active == A && n == 2 && step == Step::PrecombatMain => Some(1),
        CaseId::G02a | CaseId::G02b if step == Step::PrecombatMain => match (active, n) {
            (A, 2) | (B, 3) => Some(1),
            (A, 3) | (B, 4) => Some(2),
            _ => None,
        },
        CaseId::G02a | CaseId::G02b if active == A && n == 5 && step == Step::PostcombatMain => {
            Some(3)
        }
        _ => None,
    };
    if let Some(index) = lion {
        for source in 1..=2 {
            push(
                entries,
                case,
                ScriptPosition { turn, active, step },
                active,
                &format!("tap.{source}"),
                Recipe::Tap(format!("{}.P{source}", seat_name(active)?)),
            );
        }
        let alias = format!("{}.L{index}", seat_name(active)?);
        push(
            entries,
            case,
            ScriptPosition { turn, active, step },
            active,
            "cast",
            Recipe::Cast(alias.clone()),
        );
        rounds(entries, case, turn, active, step, Some(alias), "resolve")?;
    }
    Ok(())
}
fn attack_recipe(case: CaseId, active: SeatId, n: u64) -> (Vec<String>, Vec<String>) {
    match case {
        CaseId::G01 if active == A && n >= 3 => (vec!["A.L1".into()], vec!["A.L1".into()]),
        CaseId::G02a | CaseId::G02b if active == A => match n {
            3 => (vec!["A.L1".into()], vec![]),
            4 => (range("A", "L", 1, 2), vec![]),
            5 => (range("A", "L", 1, 2), range("A", "L", 1, 2)),
            6..=15 => (vec!["A.L3".into()], vec!["A.L3".into()]),
            _ => (vec![], vec![]),
        },
        CaseId::G02a | CaseId::G02b if active == B && n == 4 => (vec!["B.L1".into()], vec![]),
        _ => (vec![], vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn depletion_script_expands_all_real_passes_and_discards() {
        let policy = Policy::new(
            &Fixture::new(
                CaseId::G03,
                crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(policy.script_len(), 1820);
        let Strategy::Historical(entries) = &policy.strategy else {
            panic!("historical strategy required");
        };
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(entry.recipe, Recipe::Discard { .. }))
                .count(),
            106
        );
        assert_eq!(entries[1818].actor, B);
        assert_eq!(entries[1819].actor, A);
        assert_eq!(entries[1819].turn, Some((108, Step::Upkeep)));
    }
    #[test]
    fn prepared_input_authority_and_acceptance_consume_exactly_once() {
        let fixture = Fixture::new(
            CaseId::G01,
            crate::creature_types::synthetic_vocabulary_for_rules(
                "synthetic-policy-authority-unit",
            ),
        )
        .unwrap();
        let state = GameState::initialize_opening(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.clone(),
        )
        .unwrap()
        .into_state();
        let mut policy = Policy::new(&fixture).unwrap();
        let prepared = policy.prepare(&state).unwrap().unwrap();
        let mut wrong = prepared.clone();
        wrong.input.actor = super::B;
        assert!(policy.issue(&state, wrong).is_err());
        assert_eq!(policy.consumed(), 0);
        let stale = prepared.clone();
        let input = policy.issue(&state, prepared).unwrap();
        assert!(policy.prepare(&state).is_err());
        assert!(policy.issue(&state, stale.clone()).is_err());
        let candidate = state.advance(input.clone()).unwrap();
        let mut changed = input.clone();
        changed.basis.revision += 1;
        assert!(policy.accept(&changed, &candidate).is_err());
        assert_eq!(policy.consumed(), 0);
        policy.accept(&input, &candidate).unwrap();
        assert_eq!(policy.consumed(), 1);
        assert!(policy.accept(&input, &candidate).is_err());
        assert_eq!(policy.consumed(), 1);
        assert!(policy.issue(candidate.state(), stale).is_err());
        assert_eq!(policy.consumed(), 1);
        assert!(policy.prepare(candidate.state()).unwrap().is_some());
    }
}
