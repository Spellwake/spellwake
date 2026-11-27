//! Finite input-recipe caller of the real root. Saved inputs are reexecuted verbatim.
use super::capture::Capture;
use super::{
    input::{self, Action, Binding, Join, Reference, Selected},
    program, projection,
};
use crate::current_profile::{
    Append, CallPoint, Phase, Recorder, ReplayEvidence, SnapshotOwner, SourceCapture,
};
use serde::Deserialize;
use serde_json::{Value, json};
use spellwake_engine::{
    activated::ActivatedAbilityId,
    combat::{AttackDeclaration, BlockDeclaration, CombatDamageAssignment},
    damage::{DamageEffectCreation, DamageEffectId, DamageOrigin, DamageRecipient},
    effects::CopyTargetChoice,
    game::{
        DecisionId, DecisionReply, GameBasis, GameCommand, GameError, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    mana::{ManaAbilityRef, ManaPayment, ManaSpend},
    object_identity::{CardId, ObjectId},
    occurrence::{RulesOccurrence, RulesRecord, TriggerInstanceId, TriggerRelated},
    opening::MulliganChoice,
    priority::PriorityPhase,
    stack_window::{StackEntryId, WindowPhase},
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
#[path = "cast_join_tests.rs"]
mod cast_join_tests;

#[derive(Default)]
struct Bindings {
    objects: BTreeMap<String, ObjectId>,
    entries: BTreeMap<String, StackEntryId>,
    instances: BTreeMap<String, TriggerInstanceId>,
    productions: BTreeMap<String, Value>,
    damage_effects: BTreeMap<String, DamageEffectId>,
    production_joins: BTreeMap<String, (ObjectId, ManaAbilityRef)>,
}
// Test recipe runner command enum kept inline for concise pattern matching.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum RootCall {
    Advance(GameInput),
    SupplyEntropy { basis: GameBasis, words: Vec<u64> },
}
impl From<GameInput> for RootCall {
    fn from(value: GameInput) -> Self {
        Self::Advance(value)
    }
}
impl RootCall {
    fn execute(&self, source: &GameState) -> Result<NativeCandidate, GameError> {
        match self {
            Self::Advance(value) => source.advance(value.clone()),
            Self::SupplyEntropy { basis, words } => source.supply_entropy(basis, words.clone()),
        }
    }
    fn qualification_call(&self) -> projection::QualificationCall<'_> {
        match self {
            Self::Advance(input) => projection::QualificationCall::GameInput(input),
            Self::SupplyEntropy { basis, words } => {
                projection::QualificationCall::GameSupplyEntropy { basis, words }
            }
        }
    }
    fn view(&self) -> Value {
        match self {
            Self::Advance(value) => projection::input(value),
            Self::SupplyEntropy { basis, words } => {
                json!({"tag":"SupplyEntropy","basis":projection::basis(basis),"words":words})
            }
        }
    }
}
#[derive(Clone)]
enum Observation {
    ReadOnly {
        metadata: Value,
        methods: Vec<String>,
        repeat_count: usize,
    },
    ForkEquality {
        metadata: Value,
    },
}
enum Saved {
    Observation {
        request: Observation,
        record: Value,
    },
    Snapshot {
        name: String,
        record: Value,
    },
    Input {
        input: Box<RootCall>,
        source: Option<String>,
        adopt: bool,
        metadata: Value,
        observed: Box<Result<NativeCandidate, GameError>>,
        record: Value,
    },
}
struct Driver<'a> {
    selected: &'a Selected,
    capture: Option<Capture>,
    current_pointer: String,
    state: GameState,
    bindings: Bindings,
    snapshots: BTreeMap<String, GameState>,
    saved: Vec<Saved>,
    records: Vec<Value>,
    count: usize,
    last_candidate: NativeCandidate,
    literal_replies: BTreeMap<usize, (String, Vec<Value>, String)>,
}
pub(super) struct Planned {
    ordinal: usize,
    path: Vec<usize>,
    action: Action,
}
fn literal_reply_baselines(plan: &[Planned]) -> BTreeMap<usize, (String, Vec<Value>, String)> {
    let mut result = BTreeMap::new();
    for (index, row) in plan.iter().enumerate() {
        if !matches!(row.action, Action::AttemptReplyWithoutAdoption { .. }) {
            continue;
        }
        for next in &plan[index + 1..] {
            match &next.action {
                Action::ReplyExactCurrentFrame {
                    kind, items, actor, ..
                } => {
                    result.insert(row.ordinal, (kind.clone(), items.clone(), actor.clone()));
                    break;
                }
                Action::AttemptReplyWithoutAdoption { .. }
                | Action::AttemptWithoutAdoption { .. }
                | Action::SaveSnapshot { .. } => {}
                _ => break,
            }
        }
    }
    result
}
fn check_stop(stop: &input::Stop, state: &GameState) -> Result<(), String> {
    match (&stop.turn, &stop.step, state.turn()) {
        (Some(number), Some(step), Some(turn))
            if turn.position().number == *number
                && turn.position().step == program::step(step)?
                && stop
                    .cleanup_iteration
                    .is_none_or(|iteration| turn.position().cleanup_iteration == iteration) => {}
        (None, None, _) if stop.cleanup_iteration.is_none() => {}
        _ => return Err("finite recipe ended outside its literal calendar stop".into()),
    }
    match stop.kind.as_str() {
        "Ready"
            if state.pending_decision().is_none()
                && matches!(
                    state.execution().window().phase(),
                    WindowPhase::Priority(PriorityPhase::Ready { .. })
                ) =>
        {
            Ok(())
        }
        "AfterInitialization" | "AfterLastRootInput" => Ok(()),
        "Outcome" if state.outcome().is_some() && state.pending_decision().is_none() => Ok(()),
        "PendingDamage"
            if matches!(
                state.pending_decision().map(|value| value.frame()),
                Some(RuleFrame::Damage(_))
            ) =>
        {
            Ok(())
        }
        "PendingMulligan"
            if matches!(
                state.pending_decision().map(|decision| decision.frame()),
                Some(RuleFrame::Opening(
                    spellwake_engine::opening::OpeningDecision::Declare { .. }
                ))
            ) =>
        {
            Ok(())
        }
        _ => Err("finite recipe exhausted outside its literal prepared stop boundary".into()),
    }
}
fn observation(
    request: &Observation,
    state: &GameState,
    last_candidate: &NativeCandidate,
    results: &[&Result<NativeCandidate, GameError>],
    aliases: &crate::packet::AliasContext,
) -> Result<Value, String> {
    match request {
        Observation::ForkEquality { metadata } => {
            let [a, b] = results else {
                return Err("fork equality lacks two actual Results".into());
            };
            Ok(
                json!({"tag":"SameInputForks","metadata":metadata,"full_result_eq":a==b,"adopted":false}),
            )
        }
        Observation::ReadOnly {
            metadata,
            methods,
            repeat_count,
        } => {
            let retained = state.clone();
            let retained_candidate = last_candidate.clone();
            let mut rows = Vec::new();
            for repeat in 0..*repeat_count {
                for method in methods {
                    let value=match method.as_str(){
                    "basis"=>projection::basis(state.basis()),
                    "pending_decision"=>state.pending_decision().map(|decision|Ok::<Value,String>(json!({"id":decision.id().0.to_string(),"basis":projection::basis(decision.basis()),"chooser":decision.chooser().0,"frame":projection::frame(decision.frame(),state)?}))).transpose()?.unwrap_or(Value::Null),
                    "execution"=>projection::effect(state.execution(),aliases)?,
                    "candidate.into_state_on_clone"=>{let owned=last_candidate.clone().into_state();json!({"state":projection::state(&owned,aliases)?,"full_state_eq_current":owned==*state})},
                    _=>return Err("unknown read-only operation".into()),
                };
                    rows.push(json!({"repeat":repeat,"method":method,"value":value}));
                }
            }
            Ok(
                json!({"tag":"ReadOnlyOperations","metadata":metadata,"observations":rows,"retained_source_full_state_eq":*state==retained,"retained_candidate_full_eq":*last_candidate==retained_candidate}),
            )
        }
    }
}
fn validate_choice(kind: &str, items: &[Value]) -> Result<(), String> {
    match kind {
        "TimestampOrder" => {
            for item in items {
                program::token_member(item)?;
            }
        }
        "CreatureType" if items.len() == 1 => {}
        "TriggerTarget" if items.len() == 1 => {
            let _: Reference =
                serde_json::from_value(items[0].clone()).map_err(|error| error.to_string())?;
        }
        "DamageOrder" if items.len() == 1 => {
            let _: EffectReference =
                serde_json::from_value(items[0].clone()).map_err(|error| error.to_string())?;
        }
        "OrderTriggers" => {
            for value in items {
                if !value.is_string() {
                    return Err("trigger item is not a binding name".into());
                }
            }
        }
        "OrderGraveyard" | "Discard" => {
            for value in items {
                let _: Reference =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
            }
        }
        "DeclareAttackers" => {
            for value in items {
                let _: Attack =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
            }
        }
        "DeclareBlockers" => {
            for value in items {
                let _: Block =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
            }
        }
        "AssignCombatDamage" => {
            for value in items {
                let _: Assignment =
                    serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
            }
        }
        "CopyTargets" if items.len() == 1 => {
            let choice: input::CopyChoice =
                serde_json::from_value(items[0].clone()).map_err(|error| error.to_string())?;
            if matches!(choice, input::CopyChoice::Retain { target: Some(_) }) {
                return Err("Retain choice contains target".into());
            }
        }
        "ManaColor" if items.len() == 1 => {
            program::mana(items[0].as_str().ok_or("color is not text")?)?;
        }
        "Mulligan"
            if items.len() == 1 && matches!(items[0].as_str(), Some("Keep" | "Mulligan")) => {}
        _ => return Err(format!("unknown or malformed literal reply {kind}")),
    }
    Ok(())
}
fn validate_leaf(action: &Action) -> Result<(), String> {
    let guard = match action {
        Action::SaveSnapshot { combat_guard, .. }
        | Action::AttemptReplyWithoutAdoption { combat_guard, .. } => combat_guard.as_ref(),
        _ => None,
    };
    if let Some(guard) = guard {
        input::seat(&guard.actor)?;
        if !matches!(
            guard.frame.as_str(),
            "DeclareAttackers" | "DeclareBlockers" | "AssignCombatDamage"
        ) || guard
            .source
            .as_deref()
            .is_some_and(|value| value != "actual_after_action11_all_pass")
        {
            return Err("unsupported literal combat guard".into());
        }
    }
    match action {
        Action::ReplyExactCurrentFrame {
            kind, items, token, ..
        } => {
            if token != "actual_current_decision_id_and_basis" {
                return Err("unsupported literal token binding".into());
            }
            validate_choice(kind, items)?;
        }
        Action::AttemptReplyWithoutAdoption { mutation, .. } => {
            if mutation.reply_kind.is_some() != mutation.items.is_some() {
                return Err("negative kind/items must be explicit together".into());
            }
            if let (Some(kind), Some(items)) = (&mutation.reply_kind, &mutation.items) {
                validate_choice(kind, items)?;
            }
            if matches!(
                mutation.copy_choice,
                Some(input::CopyChoice::Retain { target: Some(_) })
            ) {
                return Err("negative Retain has unexpected target".into());
            }
            if let Some(actor) = &mutation.actor {
                input::seat(actor)?;
            }
            if let Some(color) = &mutation.color {
                program::mana(color)?;
            }
            let choices = usize::from(mutation.reply_kind.is_some())
                + usize::from(mutation.copy_choice.is_some())
                + usize::from(mutation.color.is_some());
            if choices > 1 {
                return Err("negative reply contains conflicting choice mutations".into());
            }
            if mutation
                .token
                .as_deref()
                .is_some_and(|token| token != "actual_current_decision_id_and_basis")
                || mutation
                    .basis_revision
                    .as_deref()
                    .is_some_and(|value| value != "actual_minus_one_checked")
                || mutation
                    .decision_id
                    .as_ref()
                    .is_some_and(|value|matches!(value,input::DecisionReference::Mutation(value) if value != "actual_plus_one_checked"))
            {
                return Err("unknown negative token/basis mutation".into());
            }
        }
        Action::ReadOnlyOperations {
            methods,
            repeat_count,
        } => {
            if *repeat_count > 1024
                || methods.iter().collect::<BTreeSet<_>>().len() != methods.len()
                || methods.iter().any(|method| {
                    !matches!(
                        method.as_str(),
                        "basis"
                            | "pending_decision"
                            | "execution"
                            | "candidate.into_state_on_clone"
                    )
                })
            {
                return Err("unsupported finite read-only method set".into());
            }
        }
        Action::SupplyEntropy { basis, .. }
            if !matches!(basis.as_str(), "actual_current_basis") =>
        {
            return Err("unknown entropy basis binding".into());
        }
        Action::AttemptWithoutAdoption { command, .. }
            if !matches!(
                command.as_ref(),
                Action::Cast { .. }
                    | Action::ActivateAbility { .. }
                    | Action::ActivateMana { .. }
                    | Action::PlayLand { .. }
                    | Action::Pass { .. }
            ) =>
        {
            return Err("nonadopted command is not one supported GameInput".into());
        }
        _ => {}
    }
    Ok(())
}
/// Expand the literal finite recipe before initialization or any root input.
pub(super) fn compile_recipe(actions: &[Action], budget: usize) -> Result<Vec<Planned>, String> {
    fn sizes(action: &Action) -> Result<(usize, usize), String> {
        validate_leaf(action)?;
        match action {
            Action::RepeatBounded { count, sequence } => {
                let (mut steps, mut inputs) = (0usize, 0usize);
                for action in sequence {
                    let (child_steps, child_inputs) = sizes(action)?;
                    steps = steps
                        .checked_add(child_steps)
                        .ok_or("recipe size overflow")?;
                    inputs = inputs
                        .checked_add(child_inputs)
                        .ok_or("recipe input count overflow")?;
                }
                Ok((
                    steps
                        .checked_mul(*count)
                        .ok_or("recipe expansion overflow")?,
                    inputs
                        .checked_mul(*count)
                        .ok_or("recipe input expansion overflow")?,
                ))
            }
            Action::PassRound { actors, role } => {
                if !matches!(
                    role.as_str(),
                    "empty_stack_calendar" | "resolve_exactly_one_current_top"
                ) {
                    return Err("unknown pass-round role".into());
                }
                Ok((1, actors.len()))
            }
            Action::SaveSnapshot { .. } | Action::ReadOnlyOperations { .. } => Ok((1, 0)),
            Action::EvaluateSameInputOnTwoForks {
                input,
                adopt_neither,
                ..
            } => {
                if !adopt_neither
                    || !matches!(input.as_ref(),Action::SupplyEntropy{basis,..} if basis=="actual_snapshot_basis")
                {
                    return Err(
                        "only the literal nonadopted snapshot entropy fork operation is admitted"
                            .into(),
                    );
                }
                Ok((1, 2))
            }
            _ => Ok((1, 1)),
        }
    }
    fn expand(action: &Action, ordinal: usize, path: Vec<usize>, out: &mut Vec<Planned>) {
        if let Action::RepeatBounded { count, sequence } = action {
            if sequence.is_empty() {
                return;
            }
            for repeat in 0..*count {
                for (index, action) in sequence.iter().enumerate() {
                    let mut path = path.clone();
                    path.extend([repeat, index]);
                    expand(action, ordinal, path, out);
                }
            }
        } else {
            out.push(Planned {
                ordinal,
                path,
                action: action.clone(),
            });
        }
    }
    let (mut steps, mut inputs) = (0usize, 0usize);
    for action in actions {
        let (child_steps, child_inputs) = sizes(action)?;
        steps = steps
            .checked_add(child_steps)
            .ok_or("recipe size overflow")?;
        inputs = inputs
            .checked_add(child_inputs)
            .ok_or("recipe input count overflow")?;
    }
    if inputs > budget {
        return Err("finite recipe exceeds literal root input budget before initialization".into());
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(steps)
        .map_err(|error| format!("recipe allocation failed: {error}"))?;
    for (index, action) in actions.iter().enumerate() {
        expand(action, index + 1, vec![], &mut result);
    }
    let mut snapshots = BTreeSet::new();
    for row in &result {
        if let Action::SaveSnapshot { name, .. } = &row.action
            && !snapshots.insert(name)
        {
            return Err("duplicate expanded saved snapshot name".into());
        }
    }
    Ok(result)
}
impl Bindings {
    fn object(
        &self,
        reference: &Reference,
        state: &GameState,
        selected: &Selected,
        source: bool,
    ) -> Result<ObjectId, String> {
        match reference {
            Reference::Captured { captured_object } => self
                .objects
                .get(captured_object)
                .copied()
                .ok_or_else(|| format!("unbound exact object {captured_object}")),
            Reference::Current {
                current_owned_action_alias,
            } if source => {
                let rows = selected
                    .fixture
                    .seed
                    .cards
                    .iter()
                    .filter(|row| row.alias == *current_owned_action_alias)
                    .collect::<Vec<_>>();
                let [card] = rows.as_slice() else {
                    return Err(format!(
                        "nonunique current action alias {current_owned_action_alias}"
                    ));
                };
                state
                    .execution()
                    .window()
                    .cards()
                    .current_identity(CardId(card.card_id))
                    .ok_or_else(|| {
                        format!("current physical card missing for {current_owned_action_alias}")
                    })
            }
            _ => Err("object reference kind is not admitted in this source/target position".into()),
        }
    }
    fn target(
        &self,
        reference: &Reference,
        state: &GameState,
        selected: &Selected,
    ) -> Result<spellwake_engine::targets::TargetRef, String> {
        use spellwake_engine::targets::TargetRef;
        match reference {
            Reference::Captured { .. } => Ok(TargetRef::Object(
                self.object(reference, state, selected, false)?,
            )),
            Reference::Entry { entry_ref } => {
                Ok(TargetRef::Stack(*self.entries.get(entry_ref).ok_or_else(
                    || format!("unbound exact entry {entry_ref}"),
                )?))
            }
            Reference::Player { player } => Ok(TargetRef::Player(input::seat(player)?)),
            _ => Err("a target cannot follow a current physical-card alias".into()),
        }
    }
    fn seed(&mut self, selected: &Selected, initial: &NativeCandidate) -> Result<(), String> {
        let mut prior = BTreeMap::<CardId, BTreeSet<ObjectId>>::new();
        for event in initial.draw_journal().events.iter() {
            if let spellwake_engine::draw::DrawEvent::Drawn { before, .. } = event {
                prior
                    .entry(
                        before
                            .card_id()
                            .ok_or("draw receipt is not a physical card")?,
                    )
                    .or_default()
                    .insert(before.id());
            }
        }
        for record in &initial.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::ZoneChange(change) = &occurrence.value
                        && let Some(card) = change.before.object.card_id()
                    {
                        prior
                            .entry(card)
                            .or_default()
                            .insert(change.before.object.id());
                    }
                }
            }
        }
        for binding in &selected.fixture.bindings {
            if let Join::Seed {
                card_id,
                require_exact_current_incarnation,
            } = binding.join
            {
                if !matches!(
                    binding.when.as_str(),
                    "after_seed_before_any_turn_work"
                        | "actual_seed_receipt_or_unchanged_initialization_object"
                ) || !require_exact_current_incarnation
                {
                    return Err("unsupported seed binding lifetime".into());
                }
                let object = match prior.get(&CardId(card_id)) {
                    Some(ids) if ids.len() == 1 => {
                        *ids.first().ok_or("empty original-object receipt")?
                    }
                    Some(_) => {
                        return Err(format!(
                            "ambiguous initial movement-before receipt for {card_id}"
                        ));
                    }
                    None => initial
                        .state()
                        .execution()
                        .window()
                        .cards()
                        .current_identity(CardId(card_id))
                        .ok_or("missing unchanged original object")?,
                };
                if self.objects.insert(binding.name.clone(), object).is_some() {
                    return Err("duplicate binding name".into());
                }
            }
        }
        Ok(())
    }
    fn observe_token_creations(
        &mut self,
        selected: &Selected,
        candidate: &NativeCandidate,
    ) -> Result<(), String> {
        for binding in &selected.fixture.bindings {
            if let Join::TokenCreation(selector) = &binding.join {
                if binding.when != "matching_actual_token_created" {
                    return Err("unsupported token birth receipt binding".into());
                }
                if self.objects.contains_key(&binding.name) {
                    continue;
                }
                let Some(creator) = self.entries.get(&selector.token_created_by_entry_ref) else {
                    continue;
                };
                let instruction = selector.instruction;
                if let Some(id) = crate::receipt_joins::token_creation_at(
                    candidate.rules_journal(),
                    *creator,
                    instruction,
                    selector.birth_ordinal,
                )? {
                    self.objects.insert(binding.name.clone(), id);
                }
            }
        }
        Ok(())
    }
    fn observe(&mut self, selected: &Selected, candidate: &NativeCandidate) -> Result<(), String> {
        self.observe_token_creations(selected, candidate)?;
        if let Some(value) = candidate.mana_production() {
            let names = self
                .production_joins
                .iter()
                .filter(|(name, (source, ability))| {
                    !self.productions.contains_key(*name)
                        && *source == value.source
                        && *ability == value.ability
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if names.len() > 1 {
                return Err("ambiguous actual mana-production receipt join".into());
            }
            if let Some(name) = names.first() {
                self.productions.insert(name.clone(),json!({"source":projection::opaque(value.source),"ability":crate::mana_ref::view(value.ability),"controller":value.controller.0,"produced":value.produced.iter().map(|(kind,amount)|(projection::opaque(kind),amount)).collect::<BTreeMap<_,_>>()}));
            }
        }
        for binding in &selected.fixture.bindings {
            match &binding.join {
                Join::TokenCreation { .. } => {}
                Join::BirthDiscovery {
                    source_object_ref,
                    triggered_ability_id,
                    related_token_after_ref,
                    require_unique,
                } => {
                    if binding.when != "first_matching_actual_birth_discovery_in_this_recipe"
                        || !require_unique
                    {
                        return Err("unsupported token birth discovery binding".into());
                    }
                    if self.instances.contains_key(&binding.name) {
                        continue;
                    }
                    let Some(source) = self.objects.get(source_object_ref) else {
                        continue;
                    };
                    let Some(related) = self.objects.get(related_token_after_ref) else {
                        continue;
                    };
                    if let Some(id) = crate::receipt_joins::birth_discovery(
                        candidate.rules_journal(),
                        *source,
                        *triggered_ability_id,
                        *related,
                    )? {
                        self.instances.insert(binding.name.clone(), id);
                    }
                }
                Join::Seed { .. } => {}
                Join::PermanentEntry {
                    actual_permanent_entry_of,
                    field,
                    require_unique,
                } => {
                    if binding.when != "after_first_permanent_resolution"
                        || !matches!(field.as_str(), "before.id" | "after.id")
                        || !*require_unique
                    {
                        return Err("unsupported permanent-entry binding".into());
                    }
                    if self.objects.contains_key(&binding.name) {
                        continue;
                    }
                    if let Some(entry) = self.entries.get(actual_permanent_entry_of)
                        && let Some(receipt) = candidate
                            .permanent_entry()
                            .filter(|receipt| receipt.entry == *entry)
                    {
                        self.objects.insert(
                            binding.name.clone(),
                            if field == "before.id" {
                                receipt.before.id()
                            } else {
                                receipt.after.id()
                            },
                        );
                    }
                }
                Join::TriggerPlacement {
                    trigger_placed_instance_ref,
                    ..
                } => {
                    if self.entries.contains_key(&binding.name) {
                        continue;
                    }
                    let Some(instance) = self.instances.get(trigger_placed_instance_ref) else {
                        continue;
                    };
                    let found = candidate
                        .rules_journal()
                        .records
                        .iter()
                        .filter_map(|row| match row {
                            RulesRecord::TriggerPlaced(placement)
                                if placement.instance == *instance =>
                            {
                                Some(placement.entry)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if found.len() > 1 {
                        return Err("nonunique actual trigger placement receipt".into());
                    }
                    if let Some(entry) = found.first() {
                        self.entries.insert(binding.name.clone(), *entry);
                    }
                }
                Join::CopyCreation {
                    copy_created_by_entry_ref,
                    instruction,
                    original_entry_ref,
                    ..
                } => {
                    if self.entries.contains_key(&binding.name) {
                        continue;
                    }
                    let (Some(creator), Some(original)) = (
                        self.entries.get(copy_created_by_entry_ref),
                        self.entries.get(original_entry_ref),
                    ) else {
                        continue;
                    };
                    let instruction = input::canonical_usize(instruction)?;
                    let found=candidate.rules_journal().records.iter().filter_map(|row|match row{RulesRecord::Event(event) if matches!(event.cause,spellwake_engine::occurrence::EventCause::Instruction{entry,instruction:actual} if entry==*creator&&actual==instruction)=>Some(event),_=>None}).flat_map(|event|event.occurrences.iter()).filter_map(|row|match &row.value{RulesOccurrence::CopyCreated{entry,original:actual,..} if actual==original=>Some(*entry),_=>None}).collect::<Vec<_>>();
                    if found.len() > 1 {
                        return Err("nonunique actual copy creation receipt".into());
                    }
                    if let Some(entry) = found.first() {
                        self.entries.insert(binding.name.clone(), *entry);
                    }
                }
                Join::DamageEffectCreation {
                    damage_effect_created_by_entry_ref,
                    instruction,
                    effect_kind,
                    ..
                } => {
                    if self.damage_effects.contains_key(&binding.name) {
                        continue;
                    }
                    let Some(creator) = self.entries.get(damage_effect_created_by_entry_ref) else {
                        continue;
                    };
                    let instruction = input::canonical_usize(instruction)?;
                    let found = candidate
                        .rules_journal()
                        .records
                        .iter()
                        .filter_map(|row| match row {
                            RulesRecord::Event(event) => Some(event),
                            _ => None,
                        })
                        .flat_map(|event| event.occurrences.iter())
                        .filter_map(|row| match &row.value {
                            RulesOccurrence::DamageEffectCreated(
                                DamageEffectCreation::Prevention {
                                    shield,
                                    instruction: actual,
                                },
                            ) if effect_kind == "Prevention"
                                && shield.creator == *creator
                                && *actual == instruction =>
                            {
                                Some(DamageEffectId::Prevention(shield.id))
                            }
                            RulesOccurrence::DamageEffectCreated(
                                DamageEffectCreation::Multiplier {
                                    multiplier,
                                    instruction: actual,
                                },
                            ) if effect_kind == "Multiplier"
                                && multiplier.creator == *creator
                                && *actual == instruction =>
                            {
                                Some(DamageEffectId::Multiplier(multiplier.id))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    if found.len() > 1 {
                        return Err("nonunique actual damage effect creation receipt".into());
                    }
                    if let Some(effect) = found.first() {
                        self.damage_effects.insert(binding.name.clone(), *effect);
                    }
                }
                Join::ResolutionExit {
                    resolution_entry_ref,
                    actual_zone_move_before_ref,
                    field,
                    require_unique,
                } => {
                    if binding.when != format!("after_{resolution_entry_ref}_resolution")
                        || field != "after.id"
                        || !require_unique
                    {
                        return Err("unsupported actual resolution movement join".into());
                    }
                    if self.objects.contains_key(&binding.name) {
                        continue;
                    }
                    let Some(entry) = self.entries.get(resolution_entry_ref) else {
                        continue;
                    };
                    let before = self
                        .objects
                        .get(actual_zone_move_before_ref)
                        .ok_or("unbound resolution movement original object")?;
                    let found=candidate.rules_journal().records.iter().filter_map(|record|match record{RulesRecord::Event(event) if matches!(event.cause,spellwake_engine::occurrence::EventCause::Instruction{entry:actual,..} if actual==*entry)=>Some(event),_=>None}).flat_map(|event|event.occurrences.iter()).filter_map(|row|match &row.value{RulesOccurrence::ZoneChange(change) if change.before.object.id()==*before=>Some(change.after.object.id()),_=>None}).collect::<Vec<_>>();
                    if found.len() > 1 {
                        return Err("nonunique actual resolution movement receipt".into());
                    }
                    if let Some(id) = found.first() {
                        self.objects.insert(binding.name.clone(), *id);
                    }
                }
                Join::Discovery {
                    source_object_ref,
                    triggered_ability_id,
                    related_object_ref,
                    step,
                    require_unique,
                } => {
                    if binding.when != "first_matching_actual_discovery_in_this_recipe"
                        || !*require_unique
                    {
                        return Err("unsupported discovery binding".into());
                    }
                    if self.instances.contains_key(&binding.name) {
                        continue;
                    }
                    let source = *self
                        .objects
                        .get(source_object_ref)
                        .ok_or_else(|| format!("missing discovery source {source_object_ref}"))?;
                    let found = crate::receipt_joins::discovery(
                        candidate.rules_journal(),
                        source,
                        *triggered_ability_id,
                        &format!("ambiguous actual discovery join {}", binding.name),
                        |relation| {
                            Ok(match (relation, related_object_ref, step) {
                                (
                                    TriggerRelated::Step { position, active },
                                    None,
                                    Some(expected),
                                ) => {
                                    position.number == expected.turn
                                        && position.step == program::step(&expected.step)?
                                        && *active == input::seat(&expected.active)?
                                }
                                (relation, Some(name), None) => {
                                    let expected = *self
                                        .objects
                                        .get(name)
                                        .ok_or_else(|| format!("missing related object {name}"))?;
                                    match relation {
                                        TriggerRelated::SpellCast(value) => {
                                            value.spell().object.id() == expected
                                        }
                                        TriggerRelated::ZoneChange { before, .. }
                                        | TriggerRelated::Discard { before, .. } => {
                                            before.object.id() == expected
                                        }
                                        TriggerRelated::StatusChange(change) => {
                                            change.before.id() == expected
                                        }
                                        TriggerRelated::Step { .. } => false,
                                        TriggerRelated::TokenCreated { .. } => false,
                                        TriggerRelated::CombatDamageToPlaneswalker { .. }
                                        | TriggerRelated::SubtypeCombatDamage { .. }
                                        | TriggerRelated::AttachedCreatureDamaged { .. }
                                        | TriggerRelated::DamageToOpponent { .. }
                                        | TriggerRelated::DamageDealtToPermanent { .. }
                                        | TriggerRelated::ReflexiveDiscardDamage { .. }
                                        | TriggerRelated::DamageReceived { .. }
                                        | TriggerRelated::CombatDamageToPlayer { .. }
                                        | TriggerRelated::ManaSource(_)
                                        | TriggerRelated::AttackGroup { .. }
                                        | TriggerRelated::CardDrawn { .. }
                                        | TriggerRelated::PlayerLost(_)
                                        | TriggerRelated::SagaChapter { .. }
                                        | TriggerRelated::CounterSource(_)
                                        | TriggerRelated::Combatant(_)
                                        | TriggerRelated::DeclaredSubtypeAttackers(_)
                                        | TriggerRelated::AttackedWithTotalPower(_)
                                        | TriggerRelated::AttackedAlone(_) => {
                                            return Err(
                                                "historical recipe excludes solo attack triggers"
                                                    .into(),
                                            );
                                        }
                                        TriggerRelated::LibraryAction(_) => return Err(
                                            "root recipe profile excludes library action triggers"
                                                .into(),
                                        ),
                                        TriggerRelated::GraveyardReturnPaid { .. }
                                        | TriggerRelated::AbilityForCopy(_)
                                        | TriggerRelated::SpellCopied(_)
                                        | TriggerRelated::TargetingEntry(_)
                                        | TriggerRelated::TargetedPermanent(_)
                                        | TriggerRelated::DamageSource(_)
                                        | TriggerRelated::StateTriggerSource(_)
                                        | TriggerRelated::FinalSagaChapter { .. }
                                        | TriggerRelated::LifeChanged(_)
                                        | TriggerRelated::Phased(_)
                                        | TriggerRelated::Mutated(_)
                                        | TriggerRelated::MiracleRevealed(_)
                                        | TriggerRelated::ReflexiveTrigger { .. }
                                        | TriggerRelated::LeftGame(_) => false,
                                    }
                                }
                                _ => false,
                            })
                        },
                    )?;
                    if let Some(id) = found {
                        self.instances.insert(binding.name.clone(), id);
                    }
                }
            }
        }
        Ok(())
    }
    fn view(&self) -> Value {
        json!({"objects":self.objects.iter().map(|(name,id)|(name,projection::opaque(id))).collect::<BTreeMap<_,_>>(),"entries":self.entries.iter().map(|(name,id)|(name,projection::opaque(id))).collect::<BTreeMap<_,_>>(),"instances":self.instances.iter().map(|(name,id)|(name,id.sequence().to_string())).collect::<BTreeMap<_,_>>(),"productions":self.productions,"production_receipt_joins":self.production_joins.iter().map(|(name,(source,ability))|(name,json!({"source":projection::opaque(source),"ability":crate::mana_ref::view(*ability)}))).collect::<BTreeMap<_,_>>()})
    }
}
fn payment(rows: &[input::Spend]) -> Result<ManaPayment, String> {
    Ok(ManaPayment {
        from_lots: Default::default(),
        spend: rows
            .iter()
            .map(|row| {
                Ok(ManaSpend {
                    mana_type: program::mana(&row.kind)?,
                    amount: row.amount.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attack {
    attacker: Reference,
    defender: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    blocker: Reference,
    attacker: Reference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assignment {
    source: Reference,
    target: Reference,
    amount: spellwake_contracts::RulesInteger,
}
impl Driver<'_> {
    fn literal_mana_reply(&self, state: &GameState) -> Result<DecisionReply, String> {
        let decision = state
            .pending_decision()
            .ok_or("negative reply source has no current decision")?;
        let RuleFrame::ManaColor(frame) = decision.frame() else {
            return Err("negative reply baseline has no selected supported frame recipe".into());
        };
        state
            .execution()
            .window()
            .cards()
            .get(frame.source())
            .ok_or("actual mana source missing")?;
        let mut rows = Vec::new();
        for row in &self.selected.fixture.choice_tables {
            if row.context != "ManaColor" {
                return Err("unsupported literal choice context".into());
            }
            if self
                .bindings
                .object(&row.source, state, self.selected, true)?
                == frame.source()
            {
                rows.push(row);
            }
        }
        let [row] = rows.as_slice() else {
            return Err("negative reply needs one literal source-bound choice table".into());
        };
        if input::seat(&row.actor)? != decision.chooser() {
            return Err("literal mana table actor differs from issued chooser".into());
        }
        if row.forge_callback != "chooseColor" || !row.retain_actual_offered_mask_and_order {
            return Err("unsupported literal mana-choice descriptor".into());
        }
        let offered = row
            .allowed_color_set
            .iter()
            .map(|item| program::mana(item))
            .collect::<Result<BTreeSet<_>, String>>()?;
        if offered.len() != row.allowed_color_set.len() || &offered != frame.colors() {
            return Err("literal mana membership set differs from actual issued domain".into());
        }
        Ok(DecisionReply::ManaColor(program::mana(&row.reply)?))
    }

    fn choice(
        &self,
        state: &GameState,
        kind: &str,
        items: &[Value],
    ) -> Result<DecisionReply, String> {
        let current = state
            .pending_decision()
            .ok_or("recipe reply reached no actual decision")?;
        let object = |value: &Value| {
            let reference: Reference =
                serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
            self.bindings
                .object(&reference, state, self.selected, false)
        };
        Ok(match (kind, current.frame()) {
            ("TimestampOrder", RuleFrame::TimestampOrder(_)) => DecisionReply::TimestampOrder(
                items
                    .iter()
                    .map(program::token_member)
                    .collect::<Result<_, _>>()?,
            ),
            (
                "CreatureType",
                RuleFrame::CreatureType(_)
                | RuleFrame::PromptChoice(spellwake_engine::toolbox::PromptChoiceFrame {
                    domain: spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { .. },
                    ..
                }),
            ) => lower_creature_type_reply(state, items, &self.selected.vocabulary)?,
            (
                "DamageOrder",
                RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(frame)),
            ) if items.len() == 1 => {
                let row: EffectReference =
                    serde_json::from_value(items[0].clone()).map_err(|error| error.to_string())?;
                let expected = *self
                    .bindings
                    .damage_effects
                    .get(&row.effect_ref)
                    .ok_or("unbound actual damage effect creation")?;
                let matches = frame
                    .applications()
                    .iter()
                    .filter(|application| application.effect == expected)
                    .collect::<Vec<_>>();
                let [application] = matches.as_slice() else {
                    return Err(
                        "effect receipt does not identify exactly one actual offered application"
                            .into(),
                    );
                };
                DecisionReply::DamageOrder(**application)
            }
            ("TriggerTarget", RuleFrame::TriggerTarget(_)) if items.len() == 1 => {
                let reference: Reference =
                    serde_json::from_value(items[0].clone()).map_err(|error| error.to_string())?;
                DecisionReply::TriggerTarget(self.bindings.target(
                    &reference,
                    state,
                    self.selected,
                )?)
            }
            ("OrderTriggers", RuleFrame::Triggers(_)) => DecisionReply::OrderTriggers(
                items
                    .iter()
                    .map(|item| {
                        let name = item
                            .as_str()
                            .ok_or("trigger permutation item is not a binding name")?;
                        self.bindings
                            .instances
                            .get(name)
                            .copied()
                            .ok_or_else(|| format!("unbound actual trigger instance {name}"))
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            (
                "OrderGraveyard",
                RuleFrame::Checks(spellwake_engine::checks::CheckDecision::GraveyardOrder(_)),
            ) => DecisionReply::OrderGraveyard(
                items.iter().map(object).collect::<Result<Vec<_>, _>>()?,
            ),
            (
                "Discard",
                RuleFrame::Checks(spellwake_engine::checks::CheckDecision::CleanupDiscard(_)),
            ) => DecisionReply::Discard(items.iter().map(object).collect::<Result<Vec<_>, _>>()?),
            (
                "DeclareAttackers",
                RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Attackers(_)),
            ) => DecisionReply::DeclareAttackers(
                items
                    .iter()
                    .map(|item| {
                        let row: Attack = serde_json::from_value(item.clone())
                            .map_err(|error| error.to_string())?;
                        Ok(AttackDeclaration {
                            attacker: self.bindings.object(
                                &row.attacker,
                                state,
                                self.selected,
                                false,
                            )?,
                            defender: input::seat(&row.defender)?,
                            defending_planeswalker: None,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            (
                "DeclareBlockers",
                RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Blockers(_)),
            ) => DecisionReply::DeclareBlockers(
                items
                    .iter()
                    .map(|item| {
                        let row: Block = serde_json::from_value(item.clone())
                            .map_err(|error| error.to_string())?;
                        Ok(BlockDeclaration {
                            blocker: self.bindings.object(
                                &row.blocker,
                                state,
                                self.selected,
                                false,
                            )?,
                            attacker: self.bindings.object(
                                &row.attacker,
                                state,
                                self.selected,
                                false,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            ("CopyTargets", RuleFrame::CopyTargets(_)) if items.len() == 1 => {
                let value: input::CopyChoice =
                    serde_json::from_value(items[0].clone()).map_err(|error| error.to_string())?;
                DecisionReply::CopyTargets(self.copy_choice(state, &value)?)
            }
            (
                "AssignCombatDamage",
                RuleFrame::Combat(spellwake_engine::combat::CombatDecision::DamageAssignments(_)),
            ) => DecisionReply::AssignCombatDamage(
                items
                    .iter()
                    .map(|row| {
                        let row: Assignment = serde_json::from_value(row.clone())
                            .map_err(|error| error.to_string())?;
                        let target =
                            match self.bindings.target(&row.target, state, self.selected)? {
                                spellwake_engine::targets::TargetRef::Object(id) => {
                                    DamageRecipient::Creature(id)
                                }
                                spellwake_engine::targets::TargetRef::Player(player) => {
                                    DamageRecipient::Player(player)
                                }
                                _ => {
                                    return Err(
                                        "combat damage target cannot be a stack entry".into()
                                    );
                                }
                            };
                        Ok(CombatDamageAssignment {
                            source: self.bindings.object(
                                &row.source,
                                state,
                                self.selected,
                                false,
                            )?,
                            target,
                            amount: row.amount,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
            ("ManaColor", RuleFrame::ManaColor(_)) if items.len() == 1 => DecisionReply::ManaColor(
                program::mana(items[0].as_str().ok_or("mana color is not a string")?)?,
            ),
            (
                "Mulligan",
                RuleFrame::Opening(spellwake_engine::opening::OpeningDecision::Declare { .. }),
            ) if items.len() == 1 => DecisionReply::Mulligan(match items[0].as_str() {
                Some("Keep") => MulliganChoice::Keep,
                Some("Mulligan") => MulliganChoice::Mulligan,
                _ => return Err("unknown literal mulligan choice".into()),
            }),
            _ => {
                return Err(format!(
                    "literal reply {kind} does not match actual frame {:?}",
                    current.frame()
                ));
            }
        })
    }
    fn copy_choice(
        &self,
        state: &GameState,
        value: &input::CopyChoice,
    ) -> Result<CopyTargetChoice, String> {
        match value {
            input::CopyChoice::Retain { target: None } => Ok(CopyTargetChoice::Retain),
            input::CopyChoice::Retain { target: Some(_) } => {
                Err("Retain copy choice has an unexpected target".into())
            }
            input::CopyChoice::Change { target } => Ok(CopyTargetChoice::Change(
                self.bindings.target(target, state, self.selected)?,
            )),
        }
    }
    fn baseline_reply(&self, state: &GameState, ordinal: usize) -> Result<DecisionReply, String> {
        if matches!(
            state.pending_decision().map(|decision| decision.frame()),
            Some(RuleFrame::ManaColor(_))
        ) && !self.selected.fixture.choice_tables.is_empty()
        {
            return self.literal_mana_reply(state);
        }
        let (kind, items, actor) = self.literal_replies.get(&ordinal).ok_or(
            "negative reply has no exact later literal reply at this unchanged decision boundary",
        )?;
        let decision = state
            .pending_decision()
            .ok_or("negative reply source has no decision")?;
        if input::seat(actor)? != decision.chooser() {
            return Err(
                "later literal reply chooser differs from actual negative source frame".into(),
            );
        }
        self.choice(state, kind, items)
    }
    fn combat_guard(
        &self,
        state: &GameState,
        guard: &input::CombatGuard,
        ordinal: usize,
    ) -> Result<(), String> {
        let decision = state
            .pending_decision()
            .ok_or("combat guard reached no decision")?;
        let tag = match decision.frame() {
            RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Attackers(_)) => {
                "DeclareAttackers"
            }
            RuleFrame::Combat(spellwake_engine::combat::CombatDecision::Blockers(_)) => {
                "DeclareBlockers"
            }
            RuleFrame::Combat(spellwake_engine::combat::CombatDecision::DamageAssignments(_)) => {
                "AssignCombatDamage"
            }
            _ => return Err("combat guard reached noncombat frame".into()),
        };
        if tag != guard.frame || decision.chooser() != input::seat(&guard.actor)? {
            return Err("combat guard does not match actual issued frame".into());
        }
        if let Some(source) = &guard.source
            && (source != "actual_after_action11_all_pass"
                || ordinal != 12
                || self.records.last().is_none_or(|record| {
                    record["metadata"]["action_ordinal"] != 11
                        || record["input"]["command"]["tag"] != "Pass"
                }))
        {
            return Err("unsupported or unreached combat source guard".into());
        }
        Ok(())
    }
    fn build(&self, action: &Action, state: &GameState) -> Result<GameInput, String> {
        let (actor, command) = match action {
            Action::Pass { actor } => (input::seat(actor)?, GameCommand::Pass),
            Action::PlayLand { actor, source } => (
                input::seat(actor)?,
                GameCommand::PlayLand {
                    land: self.bindings.object(source, state, self.selected, true)?,
                },
            ),
            Action::ActivateMana {
                actor,
                source,
                ability,
                ..
            } => (
                input::seat(actor)?,
                GameCommand::ActivateMana {
                    source: self.bindings.object(source, state, self.selected, true)?,
                    ability: *ability,
                },
            ),
            Action::Cast {
                actor,
                source,
                target,
                payment: rows,
                ..
            } => {
                let spell = self.bindings.object(source, state, self.selected, true)?;
                (
                    input::seat(actor)?,
                    match target {
                        None => GameCommand::CastUntargeted {
                            spell,
                            payment: Some(payment(rows)?),
                        },
                        Some(reference) => GameCommand::CastPaid {
                            spell,
                            target: self.bindings.target(reference, state, self.selected)?,
                            payment: payment(rows)?,
                        },
                    },
                )
            }
            Action::ActivateAbility {
                actor,
                source,
                ability_id,
                target,
                payment: rows,
                ..
            } => (
                input::seat(actor)?,
                GameCommand::ActivateAbility {
                    source: self.bindings.object(source, state, self.selected, true)?,
                    ability: ActivatedAbilityId(*ability_id),
                    target: self.bindings.target(target, state, self.selected)?,
                    payment: Some(payment(rows)?),
                },
            ),
            Action::ReplyExactCurrentFrame {
                actor,
                kind,
                items,
                token,
            } => {
                if token != "actual_current_decision_id_and_basis" {
                    return Err("unsupported reply token binding".into());
                }
                let decision = state
                    .pending_decision()
                    .ok_or("no actual current decision")?;
                if decision.chooser() != input::seat(actor)? || decision.basis() != state.basis() {
                    return Err(
                        "literal reply actor/basis differs from current issued frame".into(),
                    );
                }
                (
                    input::seat(actor)?,
                    GameCommand::Reply {
                        decision: decision.id(),
                        choice: self.choice(state, kind, items)?,
                    },
                )
            }
            _ => return Err("action cannot be lowered to one root input".into()),
        };
        Ok(GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        })
    }
    fn submit(
        &mut self,
        input: impl Into<RootCall>,
        source_name: Option<String>,
        adopt: bool,
        mut metadata: Value,
        action: Option<&Action>,
    ) -> Result<(), String> {
        let input = input.into();
        if self.count
            >= self
                .selected
                .fixture
                .engine_recipes
                .native
                .budget
                .max_root_inputs
        {
            return Err("literal root input budget exhausted".into());
        }
        self.count += 1;
        metadata["source_snapshot"] = json!(source_name);
        let submitted_command = match &input {
            RootCall::Advance(value) => Some(value.command.clone()),
            _ => None,
        };
        let source = match &source_name {
            Some(name) => self
                .snapshots
                .get(name)
                .ok_or_else(|| format!("unknown saved snapshot {name}"))?,
            None => &self.state,
        };
        let retained = source.clone();
        let current_retained = self.state.clone();
        let fork = source.clone();
        let evaluated = if metadata.get("fork_index").is_some() {
            &fork
        } else {
            source
        };
        let record_index = self.records.len();
        let observation_pointer = format!("/records/{record_index}");
        let before_id = format!("call-{}-before", self.count);
        let after_id = format!("call-{}-after", self.count);
        let call_id = format!("call-{}", self.count);
        if let Some(capture) = self.capture.as_mut() {
            capture.state(
                &before_id,
                &observation_pointer,
                Phase::BeforeCall,
                evaluated,
                &format!("/records/{record_index}/source"),
                &self.selected.aliases,
            )?;
        }
        let result = input.execute(evaluated);
        let record = result_record(
            &result,
            source,
            &input,
            &metadata,
            adopt,
            (
                evaluated == &retained && source == &retained,
                self.state == current_retained,
            ),
            self.selected,
        );
        let projection_failed = record["projection_failure"] == true;
        self.records.push(record.clone());
        if let Some(capture) = self.capture.as_mut() {
            let replayed = input.execute(evaluated);
            if let Ok(candidate) = &result {
                capture.state(
                    &after_id,
                    &observation_pointer,
                    Phase::AfterCall,
                    candidate.state(),
                    &format!("/records/{record_index}/result/candidate/state"),
                    &self.selected.aliases,
                )?;
            }
            capture.recorder.record_call(
                CallPoint {
                    call_id: &call_id,
                    before_point: Some(&before_id),
                    after_point: result.as_ref().ok().map(|_| after_id.as_str()),
                    adopted: adopt && result.is_ok(),
                },
                &input.qualification_call(),
                &result,
                &replayed,
                |result| project_result(result, Some(&retained), self.selected),
                result
                    .as_ref()
                    .map(|candidate| Append::RulesJournal(candidate.rules_journal()))
                    .unwrap_or(Append::NotApplicable),
                ReplayEvidence {
                    source_eq: Some(*evaluated == retained),
                    source_capture: SourceCapture::BeforeInvocation,
                },
                &self.selected.aliases,
                Some(evaluated.execution()),
            )?;
            capture.call(&call_id);
        }
        if adopt && result.is_ok() {
            self.current_pointer = format!("/records/{record_index}/result/candidate/state");
        }
        self.saved.push(Saved::Input {
            input: Box::new(input),
            source: source_name,
            adopt,
            metadata,
            observed: Box::new(result.clone()),
            record,
        });
        if evaluated != &retained || source != &retained || self.state != current_retained {
            return Err("immutable source changed during advance".into());
        }
        if adopt && let Ok(candidate) = &result {
            self.state = candidate.state().clone();
            self.last_candidate = candidate.clone();
        }
        if projection_failed {
            return Err("actual Result retained, but its structured projection failed".into());
        }
        if adopt {
            let candidate =
                result.map_err(|error| format!("prescribed adopted input rejected: {error:?}"))?;
            if let Some(action) = action {
                match action {
                    Action::Cast {
                        bind_entry: Some(name),
                        ..
                    }
                    | Action::ActivateAbility {
                        bind_entry: Some(name),
                        ..
                    } => {
                        let entry = candidate
                            .mana_payment()
                            .ok_or("accepted admission lacks actual payment/entry receipt")?
                            .source;
                        if self.bindings.entries.insert(name.clone(), entry).is_some() {
                            return Err(format!("duplicate entry binding {name}"));
                        }
                    }
                    Action::ActivateMana {
                        bind_production: Some(name),
                        ..
                    } => {
                        let Some(GameCommand::ActivateMana { source, ability }) = submitted_command
                        else {
                            return Err(
                                "production bind is not attached to the actual mana command".into(),
                            );
                        };
                        if self
                            .bindings
                            .production_joins
                            .insert(name.clone(), (source, ability))
                            .is_some()
                        {
                            return Err("duplicate production binding name".into());
                        }
                    }
                    _ => {}
                }
            }
            self.bindings.observe(self.selected, &candidate)?;
            self.state = candidate.into_state();
        }
        Ok(())
    }
    fn guard(&self, ordinal: usize) -> Result<(), String> {
        let Some(rows) = self
            .selected
            .fixture
            .engine_recipes
            .forge
            .get("priority_actions")
            .and_then(Value::as_array)
        else {
            return Ok(());
        };
        let guards = rows
            .iter()
            .filter(|row| row["native_action_ordinal"] == ordinal)
            .filter_map(|row| row.get("guard"))
            .collect::<Vec<_>>();
        if guards.len() > 1 {
            return Err("ambiguous input-side priority guard".into());
        }
        if let Some(guard) = guards.first() {
            let turn = self
                .state
                .turn()
                .ok_or("priority guard without actual calendar")?;
            if Some(turn.position().number) != guard["turn"].as_u64()
                || Some(format!("{:?}", turn.position().step).as_str())
                    != guard["native_step"].as_str()
            {
                return Err(format!(
                    "input-side phase guard {guard} mismatches actual {:?}",
                    turn.position()
                ));
            }
        }
        Ok(())
    }
    fn entry(&self, name: &str) -> Result<StackEntryId, String> {
        self.bindings
            .entries
            .get(name)
            .copied()
            .ok_or_else(|| format!("unbound actual entry {name}"))
    }
    fn frame_guard(&self, state: &GameState, guard: &input::FrameGuard) -> Result<(), String> {
        let current = state
            .pending_decision()
            .ok_or("frame guard requires actual current decision")?;
        let valid = match (guard, current.frame()) {
            (
                input::FrameGuard::CreatureType {
                    entry_ref,
                    instruction,
                    slot,
                    chooser,
                    vocabulary,
                    excluding,
                    domain,
                },
                RuleFrame::CreatureType(frame),
            ) => {
                let exclusions = excluding
                    .iter()
                    .map(|value| {
                        crate::creature_types::id(value.clone(), &self.selected.vocabulary)
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?;
                frame.entry() == self.entry(entry_ref)?
                    && frame.instruction() == input::canonical_usize(instruction)?
                    && frame.slot() == crate::creature_types::slot(json!(slot))?
                    && frame.chooser() == input::seat(chooser)?
                    && frame.vocabulary().to_string() == *vocabulary
                    && *frame.excluding() == exclusions
                    && exclusions.len() == excluding.len()
                    && domain == "exact_vocabulary_minus_exclusions"
            }
            (
                input::FrameGuard::CreatureType {
                    entry_ref,
                    instruction,
                    chooser,
                    vocabulary,
                    excluding,
                    domain,
                    ..
                },
                RuleFrame::PromptChoice(frame),
            ) => {
                let exclusions = excluding
                    .iter()
                    .map(|value| {
                        crate::creature_types::id(value.clone(), &self.selected.vocabulary)
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?;
                if let spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { allowed } =
                    &frame.domain
                {
                    frame.entry == self.entry(entry_ref)?
                        && frame.instruction == input::canonical_usize(instruction)?
                        && frame.chooser == input::seat(chooser)?
                        && self.selected.vocabulary.id().to_string() == *vocabulary
                        && exclusions.iter().all(|ex| !allowed.contains(ex))
                        && exclusions.len() == excluding.len()
                        && domain == "exact_vocabulary_minus_exclusions"
                } else {
                    false
                }
            }
            (
                input::FrameGuard::TriggerTarget {
                    chooser,
                    instance_ref,
                    entry_ref,
                    domain,
                },
                RuleFrame::TriggerTarget(frame),
            ) => {
                let expected = domain
                    .iter()
                    .map(|reference| self.bindings.target(reference, state, self.selected))
                    .collect::<Result<Vec<_>, _>>()?;
                frame.chooser() == input::seat(chooser)?
                    && frame.instance()
                        == *self
                            .bindings
                            .instances
                            .get(instance_ref)
                            .ok_or("unbound actual trigger instance")?
                    && frame.entry() == self.entry(entry_ref)?
                    && frame.domain() == expected.as_slice()
            }
            (
                input::FrameGuard::OrderTriggers {
                    chooser,
                    instance_refs,
                },
                RuleFrame::Triggers(frame),
            ) => {
                let expected = instance_refs
                    .iter()
                    .map(|name| {
                        self.bindings
                            .instances
                            .get(name)
                            .copied()
                            .ok_or("unbound actual trigger instance")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                frame.chooser() == input::seat(chooser)? && frame.instances() == expected.as_slice()
            }
            (
                input::FrameGuard::CopyTargets {
                    entry_ref,
                    instruction,
                    chooser,
                    original_entry_ref,
                },
                RuleFrame::CopyTargets(frame),
            ) => {
                frame.entry() == self.entry(entry_ref)?
                    && frame.instruction_index() == input::canonical_usize(instruction)?
                    && frame.chooser() == input::seat(chooser)?
                    && frame.original() == self.entry(original_entry_ref)?
            }
            (
                input::FrameGuard::DamageOrder {
                    entry_ref,
                    instruction,
                    chooser,
                    application_effect_refs,
                    parent,
                },
                RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(frame)),
            ) => {
                let expected = application_effect_refs
                    .iter()
                    .map(|name| {
                        self.bindings
                            .damage_effects
                            .get(name)
                            .copied()
                            .ok_or("unbound actual damage effect")
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?;
                let actual = frame
                    .applications()
                    .iter()
                    .map(|application| application.effect)
                    .collect::<BTreeSet<_>>();
                let input::ParentGuard::Resolution {
                    entry_ref: parent_entry,
                    instruction: parent_instruction,
                    locals,
                } = parent;
                let Some(spellwake_engine::effects::DamageParentView::Resolution(cursor)) =
                    state.execution().damage_parent()
                else {
                    return Err("damage guard lacks actual resolution parent".into());
                };
                let mut expected_locals = BTreeMap::new();
                for row in locals {
                    let slot = crate::creature_types::slot(json!(row.slot))?;
                    let value =
                        crate::creature_types::id(row.value.clone(), &self.selected.vocabulary)?;
                    if expected_locals.insert(slot, value).is_some() {
                        return Err("duplicate expected parent local slot".into());
                    }
                }
                frame.origin()
                    == &(DamageOrigin::Resolution {
                        entry: self.entry(entry_ref)?,
                        instruction: input::canonical_usize(instruction)?,
                    })
                    && frame.chooser() == input::seat(chooser)?
                    && actual == expected
                    && expected.len() == application_effect_refs.len()
                    && frame.applications().len() == application_effect_refs.len()
                    && cursor.entry() == self.entry(parent_entry)?
                    && cursor.instruction() == input::canonical_usize(parent_instruction)?
                    && cursor.locals() == &expected_locals
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err("literal guard differs from actual offered frame/owned parent".into())
        }
    }
    fn stop_guard(&self) -> Result<(), String> {
        let stop = &self.selected.fixture.stop;
        if let Some(guard) = &stop.ready_guard {
            let entries = guard
                .stack_entry_refs_bottom_to_top
                .iter()
                .map(|name| self.entry(name))
                .collect::<Result<Vec<_>, _>>()?;
            if self.state.execution().window().phase()
                != WindowPhase::Priority(PriorityPhase::Ready {
                    holder: input::seat(&guard.priority)?,
                })
                || self
                    .state
                    .execution()
                    .window()
                    .entries()
                    .iter()
                    .map(|entry| entry.id())
                    .collect::<Vec<_>>()
                    != entries
            {
                return Err("literal ready stop guard differs from actual root".into());
            }
        }
        if let Some(guard) = &stop.outcome_guard {
            let losses = guard
                .state_based_losses
                .iter()
                .map(|seat| input::seat(seat))
                .collect::<Result<BTreeSet<_>, _>>()?;
            if self.state.outcome()
                != Some(spellwake_engine::draw::GameOutcome::Winner(input::seat(
                    &guard.winner,
                )?))
                || self.state.execution().state_based_losses() != &losses
                || !guard.priority_forbidden
            {
                return Err("literal outcome stop differs from actual losses/outcome".into());
            }
        }
        if let Some(guard) = &stop.pending_guard {
            self.frame_guard(&self.state, guard)?;
        }
        Ok(())
    }
    fn action(&mut self, action: &Action, ordinal: usize, path: Vec<usize>) -> Result<(), String> {
        let metadata = json!({"action_ordinal":ordinal,"recipe_path":path});
        match action {
            Action::RepeatBounded { .. } => Err("unexpanded repeat reached root caller".into()),
            Action::PassRound { actors, role } => {
                if !matches!(
                    role.as_str(),
                    "empty_stack_calendar" | "resolve_exactly_one_current_top"
                ) {
                    return Err("unknown literal pass-round role".into());
                }
                if role == "resolve_exactly_one_current_top"
                    && self.state.execution().window().entries().is_empty()
                {
                    return Err("exact-one-top pass recipe reached an empty stack".into());
                }
                for (index, actor) in actors.iter().enumerate() {
                    if self.state.pending_decision().is_some()
                        || !matches!(self.state.execution().window().phase(),WindowPhase::Priority(PriorityPhase::Ready{holder}) if holder==input::seat(actor)?)
                    {
                        return Err(format!("pass actor {actor} is not actual Ready holder"));
                    }
                    let input = self.build(
                        &Action::Pass {
                            actor: actor.clone(),
                        },
                        &self.state,
                    )?;
                    self.submit(input,None,true,json!({"action_ordinal":ordinal,"recipe_path":path,"pass_index":index,"role":role}),None)?;
                }
                Ok(())
            }
            Action::SaveSnapshot {
                name,
                combat_guard,
                frame_guard,
            } => {
                if let Some(guard) = frame_guard {
                    self.frame_guard(&self.state, guard)?;
                }
                if let Some(guard) = combat_guard {
                    self.combat_guard(&self.state, guard, ordinal)?;
                }
                if self
                    .snapshots
                    .insert(name.clone(), self.state.clone())
                    .is_some()
                {
                    return Err("duplicate saved snapshot".into());
                }
                let record = json!({"tag":"SavedSnapshot","metadata":metadata,"name":name,"state":projection::state(&self.state,&self.selected.aliases)?});
                if let Some(capture) = self.capture.as_mut() {
                    let index = self.records.len();
                    capture.state(
                        &format!("record-{index}-snapshot"),
                        &format!("/records/{index}"),
                        Phase::Observation,
                        &self.state,
                        &format!("/records/{index}/state"),
                        &self.selected.aliases,
                    )?;
                }
                self.records.push(record.clone());
                self.saved.push(Saved::Snapshot {
                    name: name.clone(),
                    record,
                });
                Ok(())
            }
            Action::AttemptWithoutAdoption { snapshot, command } => {
                let source = match snapshot {
                    Some(name) => self
                        .snapshots
                        .get(name)
                        .ok_or("unknown attempt source snapshot")?,
                    None => &self.state,
                };
                let input = self.build(command, source)?;
                self.submit(input, snapshot.clone(), false, metadata, None)
            }
            Action::AttemptReplyWithoutAdoption {
                snapshot,
                mutation,
                combat_guard,
                retain_main_source,
            } => {
                let source = snapshot
                    .as_ref()
                    .map(|name| {
                        self.snapshots
                            .get(name)
                            .ok_or("unknown reply attempt snapshot")
                    })
                    .transpose()?
                    .unwrap_or(&self.state);
                if let Some(guard) = combat_guard {
                    self.combat_guard(source, guard, ordinal)?;
                }
                if let Some(name) = retain_main_source
                    && (snapshot.as_ref() != Some(name)
                        || self.snapshots.get(name) != Some(&self.state))
                {
                    return Err("negative combat reply retained main source differs from exact named snapshot".into());
                }
                let decision = source
                    .pending_decision()
                    .ok_or("reply attempt source has no actual decision")?;
                if mutation
                    .token
                    .as_deref()
                    .is_some_and(|token| token != "actual_current_decision_id_and_basis")
                {
                    return Err("unsupported negative token binding".into());
                }
                let choice = if let Some(kind) = &mutation.reply_kind {
                    let items = mutation
                        .items
                        .as_deref()
                        .ok_or("negative reply kind lacks explicit items")?;
                    if kind == "OrderGraveyard" && items.is_empty() {
                        DecisionReply::OrderGraveyard(vec![])
                    } else {
                        self.choice(source, kind, items)?
                    }
                } else if mutation.items.is_some() {
                    return Err("negative items lack explicit reply kind".into());
                } else if let Some(color) = &mutation.color {
                    DecisionReply::ManaColor(program::mana(color)?)
                } else if let Some(choice) = &mutation.copy_choice {
                    DecisionReply::CopyTargets(self.copy_choice(source, choice)?)
                } else {
                    self.baseline_reply(source, ordinal)?
                };
                let token = match &mutation.decision_id {
                    None => decision.id(),
                    Some(input::DecisionReference::Snapshot { from_snapshot }) => self
                        .snapshots
                        .get(from_snapshot)
                        .ok_or("unknown saved DecisionId snapshot")?
                        .pending_decision()
                        .ok_or("saved snapshot has no actual DecisionId")?
                        .id(),
                    Some(input::DecisionReference::Mutation(value))
                        if value == "actual_plus_one_checked" =>
                    {
                        DecisionId(
                            decision
                                .id()
                                .0
                                .checked_add(1)
                                .ok_or("decision mutation overflow")?,
                        )
                    }
                    _ => return Err("unknown decision mutation".into()),
                };
                let mut basis = source.basis().clone();
                match mutation.basis_revision.as_deref() {
                    None => {}
                    Some("actual_minus_one_checked") => {
                        basis.revision = basis
                            .revision
                            .checked_sub(1)
                            .ok_or("basis mutation underflow")?
                    }
                    _ => return Err("unknown basis mutation".into()),
                }
                let input = GameInput {
                    basis,
                    actor: mutation
                        .actor
                        .as_deref()
                        .map(input::seat)
                        .transpose()?
                        .unwrap_or(decision.chooser()),
                    command: GameCommand::Reply {
                        decision: token,
                        choice,
                    },
                };
                self.submit(input, snapshot.clone(), false, metadata, None)
            }
            Action::SupplyEntropy { basis, words } => {
                if basis != "actual_current_basis" {
                    return Err("direct entropy call lacks literal current basis binding".into());
                }
                self.submit(
                    RootCall::SupplyEntropy {
                        basis: self.state.basis().clone(),
                        words: words.clone(),
                    },
                    None,
                    true,
                    metadata,
                    None,
                )
            }
            Action::EvaluateSameInputOnTwoForks {
                snapshot,
                input,
                adopt_neither,
            } => {
                let Action::SupplyEntropy { basis, words } = input.as_ref() else {
                    return Err("unsupported fork root call".into());
                };
                if !adopt_neither || basis != "actual_snapshot_basis" {
                    return Err("unsupported fork adoption/basis contract".into());
                }
                let source = self
                    .snapshots
                    .get(snapshot)
                    .ok_or("missing fork snapshot")?;
                let call = RootCall::SupplyEntropy {
                    basis: source.basis().clone(),
                    words: words.clone(),
                };
                for fork_index in 0..2 {
                    self.submit(call.clone(),Some(snapshot.clone()),false,json!({"action_ordinal":ordinal,"recipe_path":path,"fork_index":fork_index}),None)?;
                }
                let results = self
                    .saved
                    .iter()
                    .rev()
                    .filter_map(|saved| match saved {
                        Saved::Input { observed, .. } => Some(observed.as_ref()),
                        _ => None,
                    })
                    .take(2)
                    .collect::<Vec<_>>();
                let request = Observation::ForkEquality { metadata };
                let record = observation(
                    &request,
                    &self.state,
                    &self.last_candidate,
                    &results,
                    &self.selected.aliases,
                )?;
                if let Some(capture) = self.capture.as_mut() {
                    let index = self.records.len();
                    capture.state(
                        &format!("record-{index}-observation"),
                        &format!("/records/{index}"),
                        Phase::Observation,
                        &self.state,
                        &self.current_pointer,
                        &self.selected.aliases,
                    )?;
                }
                self.records.push(record.clone());
                self.saved.push(Saved::Observation { request, record });
                Ok(())
            }
            Action::ReadOnlyOperations {
                methods,
                repeat_count,
            } => {
                let request = Observation::ReadOnly {
                    metadata,
                    methods: methods.clone(),
                    repeat_count: *repeat_count,
                };
                let record = observation(
                    &request,
                    &self.state,
                    &self.last_candidate,
                    &[],
                    &self.selected.aliases,
                )?;
                if let Some(capture) = self.capture.as_mut() {
                    let index = self.records.len();
                    capture.state(
                        &format!("record-{index}-observation"),
                        &format!("/records/{index}"),
                        Phase::Observation,
                        &self.state,
                        &self.current_pointer,
                        &self.selected.aliases,
                    )?;
                }
                self.records.push(record.clone());
                self.saved.push(Saved::Observation { request, record });
                Ok(())
            }
            _ => {
                let input = self.build(action, &self.state)?;
                self.submit(input, None, true, metadata, Some(action))
            }
        }
    }
}
fn result_record(
    result: &Result<NativeCandidate, GameError>,
    source: &GameState,
    input: &RootCall,
    metadata: &Value,
    adopt: bool,
    retained_eq: (bool, bool),
    selected: &Selected,
) -> Value {
    let source_view = projection::state(source, &selected.aliases).unwrap_or_else(
        |error| json!({"projection_error":error,"actual_private_state":format!("{source:#?}")}),
    );
    let outcome = match result {
        Ok(candidate) => {
            json!({"tag":"Ok","candidate":projection::candidate(candidate,Some(source),&selected.aliases).unwrap_or_else(|error|json!({"projection_error":error,"actual_private_candidate":format!("{candidate:#?}")}))})
        }
        Err(error) => projection::game_error(error),
    };
    let projection_failure = source_view.get("projection_error").is_some()
        || outcome["candidate"].get("projection_error").is_some();
    json!({"tag":if matches!(input,RootCall::Advance(_)){"ActualInput"}else{"ActualRootCall"},"metadata":metadata,"input":input.view(),
        "adopt_requested":adopt,"adopted":adopt&&result.is_ok(),
        "retained_source_full_state_eq":retained_eq.0,"retained_current_full_state_eq":retained_eq.1,
        "source":source_view,"result":outcome,"projection_failure":projection_failure,
        "actual_result_private_debug":format!("{result:#?}")})
}

fn replay(
    selected: &Selected,
    initial: &NativeCandidate,
    driver: &Driver<'_>,
    initializer: &input::Initializer,
) -> Result<Value, String> {
    let rebuilt = initializer
        .execute()
        .map_err(|error| format!("saved native initialization rejected: {error:?}"))?;
    let initial_full_eq = rebuilt == *initial;
    let mut last_candidate = rebuilt.clone();
    let mut state = rebuilt.into_state();
    let mut last_results = Vec::new();
    let mut snapshots = BTreeMap::new();
    let mut comparisons = Vec::new();
    let mut all = initial_full_eq;
    let mut failure = None;
    for operation in &driver.saved {
        match operation {
            Saved::Observation { request, record } => {
                let results = last_results.iter().rev().take(2).collect::<Vec<_>>();
                let actual = observation(
                    request,
                    &state,
                    &last_candidate,
                    &results,
                    &selected.aliases,
                )?;
                let eq = actual == *record;
                all &= eq;
                comparisons
                    .push(json!({"tag":"Observation","full_record_eq":eq,"actual_record":actual}));
            }
            Saved::Snapshot { name, record } => {
                snapshots.insert(name.clone(), state.clone());
                let actual = json!({"tag":"SavedSnapshot","metadata":record["metadata"],"name":name,"state":projection::state(&state,&selected.aliases)?});
                let equal = actual == *record;
                all &= equal;
                comparisons.push(json!({"tag":"Snapshot","name":name,"full_record_eq":equal}));
            }
            Saved::Input {
                input,
                source,
                adopt,
                metadata,
                observed,
                record,
            } => {
                let input = input.as_ref();
                let current_before = state.clone();
                let original = source
                    .as_ref()
                    .map(|name| snapshots.get(name).ok_or("saved replay source missing"))
                    .transpose()?
                    .unwrap_or(&state);
                let before = original.clone();
                let fork = original.clone();
                let evaluated = if metadata.get("fork_index").is_some() {
                    &fork
                } else {
                    original
                };
                let result = input.execute(evaluated);
                let actual = result_record(
                    &result,
                    original,
                    input,
                    metadata,
                    *adopt,
                    (original == &before, state == current_before),
                    selected,
                );
                last_results.push(result.clone());
                let candidate_eq = result == **observed;
                let record_eq = actual == *record;
                all &= candidate_eq && record_eq;
                comparisons.push(json!({"tag":"Input","metadata":metadata,"full_result_eq":candidate_eq,"full_record_eq":record_eq,"retained_source_full_state_eq":original==&before,"actual_record":actual}));
                if *adopt {
                    match result {
                        Ok(candidate) => {
                            last_candidate = candidate.clone();
                            state = candidate.into_state();
                        }
                        Err(error) => {
                            failure = Some(format!("saved adopted replay rejected: {error:?}"));
                            break;
                        }
                    }
                }
            }
        }
    }
    let final_state_eq = state == driver.state;
    all &= final_state_eq;
    Ok(
        json!({"saved_actual_inputs":driver.count,"saved_actual_game_inputs":driver.saved.iter().filter(|saved|matches!(saved,Saved::Input{input,..} if matches!(input.as_ref(),RootCall::Advance(_)))).count(),"saved_actual_entropy_calls":driver.saved.iter().filter(|saved|matches!(saved,Saved::Input{input,..} if matches!(input.as_ref(),RootCall::SupplyEntropy{..}))).count(),"initial_full_candidate_eq":initial_full_eq,"comparisons":comparisons,"final_full_game_state_eq":final_state_eq,"all_equal":all,"failure":failure}),
    )
}
#[cfg(test)]
pub(super) fn run(selected: &Selected) -> Value {
    run_impl(selected, None)
}
pub(super) fn run_with_recorder(selected: &Selected, recorder: Recorder) -> Value {
    run_impl(selected, Some(Capture::new(recorder)))
}
fn run_impl(selected: &Selected, mut capture: Option<Capture>) -> Value {
    let plan = match compile_recipe(
        &selected.fixture.actions,
        selected
            .fixture
            .engine_recipes
            .native
            .budget
            .max_root_inputs,
    ) {
        Ok(value) => value,
        Err(error) => {
            return json!({"status":"failed","failure":error,"records":[],"initialization":"not attempted"});
        }
    };
    let initializer = match selected.initializer() {
        Ok(value) => value,
        Err(error) => {
            return json!({"status":"failed","initialization":{"tag":"DriverError","error":error},"records":[]});
        }
    };
    if let Some(capture) = capture.as_mut()
        && let Err(error) = capture.point(
            "initialize-input",
            "/initialization",
            Phase::InitializeInput,
            SnapshotOwner::InitializeInput {
                context: Some(initializer.context()),
                seed: initializer.seed(),
                source_ref: "/selected_input/seed",
            },
            &selected.aliases,
        )
    {
        return json!({"status":"failed","failure":error,"initialization":"not attempted","records":[]});
    }
    let initialization = initializer.execute();
    let repeated_initialization = initializer.execute();
    let mut capture_failure = None;
    if let Some(capture) = capture.as_mut() {
        let result = (|| -> Result<(), String> {
            if let Ok(candidate) = &initialization {
                capture.state(
                    "initialize-after",
                    "/initialization",
                    Phase::AfterCall,
                    candidate.state(),
                    "/initialization/candidate/state",
                    &selected.aliases,
                )?;
            }
            capture.recorder.record_call(
                CallPoint {
                    call_id: "initialize",
                    before_point: Some("initialize-input"),
                    after_point: initialization.as_ref().ok().map(|_| "initialize-after"),
                    adopted: initialization.is_ok(),
                },
                &initializer.call(),
                &initialization,
                &repeated_initialization,
                |result| project_result(result, None, selected),
                initialization
                    .as_ref()
                    .map(|candidate| Append::RulesJournal(candidate.rules_journal()))
                    .unwrap_or(Append::NotApplicable),
                ReplayEvidence {
                    source_eq: None,
                    source_capture: SourceCapture::InputOnlyForInitializer,
                },
                &selected.aliases,
                None,
            )?;
            capture.call("initialize");
            Ok(())
        })();
        capture_failure = result.err();
    }
    let initialized = match initialization {
        Ok(value) => value,
        Err(error) => {
            let mut report = json!({"status":"failed","initialization":projection::game_error(&error),"records":[],"saved_input_reexecution":{"initial_full_result_eq":repeated_initialization==Err(error.clone()),"all_equal":repeated_initialization==Err(error)}});
            finish_capture(capture, &mut report);
            return report;
        }
    };
    let initial_view = match projection::candidate(&initialized, None, &selected.aliases) {
        Ok(value) => value,
        Err(error) => {
            return json!({"status":"failed","failure":error,"initialization_raw":format!("{initialized:?}")});
        }
    };
    let mut driver = Driver {
        selected,
        capture,
        current_pointer: "/initialization/candidate/state".into(),
        state: initialized.state().clone(),
        bindings: Bindings::default(),
        snapshots: BTreeMap::new(),
        saved: Vec::new(),
        records: vec![],
        count: 0,
        last_candidate: initialized.clone(),
        literal_replies: literal_reply_baselines(&plan),
    };
    let expanded_leaf_actions = plan.len();
    let mut cursor = json!({"declared_top_level_actions":selected.fixture.actions.len(),"expanded_leaf_actions":expanded_leaf_actions,"completed_leaf_actions":0,"current_action_ordinal":null,"current_recipe_path":null,"stage":"initialization_boundary"});
    let result = (|| {
        if let Some(error) = capture_failure {
            return Err(error);
        }
        let mut names = BTreeSet::new();
        for Binding { name, .. } in &selected.fixture.bindings {
            if !names.insert(name) {
                return Err("duplicate runtime binding name".into());
            }
        }
        driver.bindings.seed(selected, &initialized)?;
        driver.bindings.observe(selected, &initialized)?;
        for (index, row) in plan.into_iter().enumerate() {
            cursor["current_action_ordinal"] = json!(row.ordinal);
            cursor["current_recipe_path"] = json!(row.path);
            cursor["stage"] = json!("input_guard");
            driver.guard(row.ordinal)?;
            cursor["stage"] = json!("action");
            driver.action(&row.action, row.ordinal, row.path)?;
            cursor["completed_leaf_actions"] = json!(index + 1);
        }
        cursor["stage"] = json!("binding_and_stop_guards");
        for binding in &selected.fixture.bindings {
            let present = match binding.join {
                Join::Discovery { .. } | Join::BirthDiscovery { .. } => {
                    driver.bindings.instances.contains_key(&binding.name)
                }
                Join::TriggerPlacement { .. } | Join::CopyCreation { .. } => {
                    driver.bindings.entries.contains_key(&binding.name)
                }
                Join::DamageEffectCreation { .. } => {
                    driver.bindings.damage_effects.contains_key(&binding.name)
                }
                _ => driver.bindings.objects.contains_key(&binding.name),
            };
            if !present {
                return Err(format!("unconsumed exact binding {}", binding.name));
            }
        }
        if driver
            .bindings
            .production_joins
            .keys()
            .any(|name| !driver.bindings.productions.contains_key(name))
        {
            return Err(
                "finite recipe ended before an exact requested production receipt was observed"
                    .into(),
            );
        }
        check_stop(&selected.fixture.stop, &driver.state)?;
        driver.stop_guard()?;
        Ok::<(), String>(())
    })();
    if result.is_ok() {
        cursor["stage"] = json!("prepared_stop");
    }
    cursor["actual_root_inputs"] = json!(driver.count);
    let rerun = replay(selected, &initialized, &driver, &initializer);
    let final_state=projection::state(&driver.state,&selected.aliases).unwrap_or_else(|error|json!({"projection_error":error,"actual_private_state":format!("{:#?}",driver.state)}));
    let mut report = json!({"status":if result.is_ok()&&final_state.get("projection_error").is_none()&&rerun.as_ref().is_ok_and(|value|value["all_equal"]==true){"completed_recipe"}else{"failed"},"failure":result.err(),"initialization":{"tag":"Ok","candidate":initial_view},"records":driver.records,"bindings":driver.bindings.view(),"recipe_cursor":cursor,"final_state":final_state,"saved_input_reexecution":match rerun{Ok(value)=>value,Err(error)=>json!({"error":error})},"terminal_claim":"finite prepared stop only; no complete game or engine parity claim"});
    if let Some(capture) = driver.capture.as_mut()
        && let Err(error) = capture.state(
            "final-state",
            "/final_state",
            Phase::Observation,
            &driver.state,
            "/final_state",
            &selected.aliases,
        )
    {
        report["status"] = json!("failed");
        report["witness_failure"] = json!(error);
    }
    finish_capture(driver.capture, &mut report);
    report
}
fn project_result(
    result: &Result<NativeCandidate, GameError>,
    source: Option<&GameState>,
    selected: &Selected,
) -> Result<Value, String> {
    match result {
        Ok(candidate) => Ok(
            json!({"tag":"Ok","candidate":projection::candidate(candidate,source,&selected.aliases)?}),
        ),
        Err(error) => Ok(projection::game_error(error)),
    }
}
fn finish_capture(capture: Option<Capture>, report: &mut Value) {
    if let Some(capture) = capture {
        match capture.finish(report) {
            Ok(witness) => report["type_context_witness"] = witness,
            Err(error) => {
                report["status"] = json!("failed");
                report["witness_failure"] = json!(error);
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectReference {
    effect_ref: String,
}

/// Exact item cardinality and member lowering shared by the actual recipe caller.
pub(super) fn lower_creature_type_reply(
    state: &GameState,
    items: &[Value],
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Result<DecisionReply, String> {
    let is_prompt = match state.pending_decision().map(|pending| pending.frame()) {
        Some(RuleFrame::CreatureType(_)) => false,
        Some(RuleFrame::PromptChoice(frame))
            if matches!(
                frame.domain,
                spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { .. }
            ) =>
        {
            true
        }
        _ => return Err("recipe reply reached a different actual frame".into()),
    };
    let [value] = items else {
        return Err("CreatureType reply requires exactly one literal ID item".into());
    };
    let chosen = crate::creature_types::id(value.clone(), vocabulary)?;
    if is_prompt {
        Ok(DecisionReply::PromptChoice(
            spellwake_engine::toolbox::PromptChoiceReply::CreatureType(chosen),
        ))
    } else {
        Ok(DecisionReply::CreatureType(chosen))
    }
}
