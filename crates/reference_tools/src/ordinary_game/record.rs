//! Compact complete candidate evidence and same-input native reexecution.
use super::{
    fixture::{CaseId, Fixture, FixtureInitialization, ReportEpoch},
    policy::{Policy, PreparedInput, StopKind},
    probes,
    selectors::{Observation, Scope, Selectors},
    view::{self, *},
};
use serde::{Serialize, Serializer, ser::SerializeStruct};
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    game::{GameError, GameInput, GameState, NativeCandidate},
    object_identity::ObjectDestination,
    occurrence::RulesJournal,
};
use std::collections::BTreeMap;

#[cfg(test)]
mod observer_tests;
mod staged;
use staged::NativeTrace;

pub(super) fn equal<T: Serialize>(left: &T, right: &T) -> Result<bool, String> {
    Ok(serde_json::to_value(left).map_err(|e| e.to_string())?
        == serde_json::to_value(right).map_err(|e| e.to_string())?)
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct ContextView {
    reference: String,
    release: String,
    rules: String,
    creature_vocabulary: spellwake_contracts::creature_types::CreatureVocabularyId,
    name_rules_id: spellwake_contracts::names::NameRulesId,
    catalogue: String,
    programs: String,
    profile: String,
}
fn context(value: &super::fixture::ContextInputs) -> ContextView {
    ContextView {
        reference: value.reference.clone(),
        release: value.release.clone(),
        rules: value.rules.clone(),
        creature_vocabulary: value.creature_vocabulary,
        name_rules_id: value.name_rules_id,
        catalogue: value.catalogue.clone(),
        programs: value.programs.clone(),
        profile: value.profile.clone(),
    }
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct SourceView {
    path: String,
    sha256: String,
    uri: String,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct DefinitionView {
    reference: String,
    name: String,
    oracle_id: String,
    printing_id: String,
    set: String,
    collector: String,
    language: String,
    sources: Vec<SourceView>,
    program: serde_json::Value,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct AliasView {
    alias: String,
    card_id: String,
    owner: u32,
    definition_ref: String,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct SeedView {
    card_id: String,
    owner: u32,
    destination: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    controller: Option<u32>,
    definition_ref: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum InitializationModeView {
    Opening,
    Scheduled { start: ScheduledStartView },
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct ScheduledStartView {
    number: u64,
    step: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct SetupView {
    #[serde(skip_serializing_if = "Option::is_none")]
    initialization: Option<InitializationModeView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_battlefield_order: Option<Vec<String>>,
    participants: Vec<u32>,
    active: u32,
    combat_rules: &'static str,
    cards: Vec<SeedView>,
    mana: Vec<SeedManaView>,
    life: Vec<SeedLifeView>,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct SeedManaView {
    player: u32,
    mana_type: &'static str,
    amount: RulesInteger,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct SeedLifeView {
    player: u32,
    total: RulesInteger,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct RecipeView {
    id: String,
    owner: u32,
    native_before_aliases: Vec<String>,
    target_aliases: Vec<String>,
    native_words: Vec<String>,
    entropy_start: usize,
    entropy_end: usize,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct StaticInputs {
    case_id: String,
    game_id: String,
    contexts: Vec<ContextView>,
    definitions: Vec<DefinitionView>,
    aliases: Vec<AliasView>,
    setup: SetupView,
    random_recipes: Vec<RecipeView>,
    native_entropy_words: Vec<String>,
}
fn seed_view(
    fixture: &Fixture,
    value: &spellwake_engine::effects::SeedCard,
) -> Result<SeedView, String> {
    let alias = fixture
        .aliases
        .get(&value.card)
        .ok_or("seed lacks exact physical alias")?;
    let definition = fixture
        .definitions
        .iter()
        .find(|v| v.reference == alias.definition_ref)
        .ok_or("seed lacks definition")?;
    if alias.card != value.card || alias.owner != value.owner || definition.program != value.program
    {
        return Err("actual seed differs from its recorded alias/owner/program definition".into());
    }
    let prepared = matches!(
        fixture.case.observation_contract().report_epoch,
        ReportEpoch::Report2 | ReportEpoch::Report3 | ReportEpoch::Report4
    ) && matches!(fixture.initialization, FixtureInitialization::Scheduled(_));
    Ok(SeedView {
        card_id: value.card.0.to_string(),
        owner: value.owner.0,
        destination: match value.destination {
            ObjectDestination::LibraryBottom => "library_bottom",
            ObjectDestination::LibraryTop if prepared => "library_top",
            ObjectDestination::Hand if prepared => "hand",
            ObjectDestination::Graveyard if prepared => "graveyard",
            // Report4 reduced recipes retain unused source-pool members in
            // exile. This records their explicit initial premise, not a move.
            ObjectDestination::Exile
                if prepared
                    && fixture.case.observation_contract().report_epoch == ReportEpoch::Report4 =>
            {
                "exile"
            }
            ObjectDestination::Battlefield { .. } if prepared => "battlefield",
            _ => return Err("ordinary fixture seed is not library-only".into()),
        },
        controller: match value.destination {
            ObjectDestination::Battlefield { controller } if prepared => Some(controller.0),
            _ => None,
        },
        definition_ref: definition.reference.clone(),
    })
}
fn static_inputs(fixture: &Fixture) -> Result<StaticInputs, String> {
    fixture.require_ordinary_execution()?;
    let words = fixture
        .shuffles
        .iter()
        .flat_map(|v| v.words.iter().copied())
        .collect::<Vec<_>>();
    if fixture.setup.entropy != spellwake_engine::shuffle::EntropyTape::new(words.clone()) {
        return Err("actual constructor entropy differs from recorded input words".into());
    }
    let (probe_inputs, _) = fixture.probe_context()?;
    Ok(StaticInputs {
        case_id: fixture.case.as_str().into(),
        game_id: fixture.game.0.to_string(),
        contexts: vec![context(&fixture.context_inputs), context(&probe_inputs)],
        definitions: fixture
            .definitions
            .iter()
            .map(|v| {
                Ok(DefinitionView {
                    reference: v.reference.clone(),
                    name: v.name.clone(),
                    oracle_id: v.oracle_id.clone(),
                    printing_id: v.printing_id.clone(),
                    set: v.set.clone(),
                    collector: v.collector.clone(),
                    language: v.language.clone(),
                    sources: v
                        .sources
                        .iter()
                        .map(|v| SourceView {
                            path: v.path.clone(),
                            sha256: v.sha256.clone(),
                            uri: v.uri.clone(),
                        })
                        .collect(),
                    program: crate::program::view(&v.program),
                })
            })
            .collect::<Result<_, String>>()?,
        aliases: fixture
            .aliases
            .values()
            .map(|v| AliasView {
                alias: v.alias.clone(),
                card_id: v.card.0.to_string(),
                owner: v.owner.0,
                definition_ref: v.definition_ref.clone(),
            })
            .collect(),
        setup: SetupView {
            initial_battlefield_order: matches!(
                fixture.case.observation_contract().report_epoch,
                ReportEpoch::Report2 | ReportEpoch::Report3 | ReportEpoch::Report4
            )
            .then(|| {
                fixture
                    .setup
                    .seed
                    .initial_battlefield_order
                    .iter()
                    .map(|card| view::alias(fixture, *card))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?,
            initialization: matches!(
                fixture.case.observation_contract().report_epoch,
                ReportEpoch::Report2 | ReportEpoch::Report3 | ReportEpoch::Report4
            )
            .then(|| match fixture.initialization {
                FixtureInitialization::Opening => InitializationModeView::Opening,
                FixtureInitialization::Scheduled(start) => InitializationModeView::Scheduled {
                    start: ScheduledStartView {
                        number: start.number,
                        step: crate::damage_trace::step_view(start.step),
                    },
                },
            }),
            participants: fixture.setup.seed.seats.iter().map(|v| v.0).collect(),
            active: fixture.setup.seed.active.0,
            combat_rules: match fixture.setup.seed.combat_rules {
                spellwake_engine::combat::CombatRules::UnrestrictedFreeForAll => {
                    "unrestricted_free_for_all"
                }
            },
            cards: fixture
                .setup
                .seed
                .cards
                .iter()
                .map(|v| seed_view(fixture, v))
                .collect::<Result<_, String>>()?,
            mana: fixture
                .setup
                .seed
                .mana
                .iter()
                .map(|v| SeedManaView {
                    player: v.player.0,
                    mana_type: view::mana_type(v.mana_type),
                    amount: v.amount.clone(),
                })
                .collect(),
            life: fixture
                .setup
                .seed
                .life
                .iter()
                .map(|v| SeedLifeView {
                    player: v.player.0,
                    total: v.total.clone(),
                })
                .collect(),
        },
        random_recipes: fixture
            .shuffles
            .iter()
            .map(|v| {
                Ok(RecipeView {
                    id: v.id.clone(),
                    owner: v.owner.0,
                    native_before_aliases: v
                        .before
                        .iter()
                        .map(|id| view::alias(fixture, *id))
                        .collect::<Result<_, _>>()?,
                    target_aliases: v
                        .target
                        .iter()
                        .map(|id| view::alias(fixture, *id))
                        .collect::<Result<_, _>>()?,
                    native_words: v.words.iter().map(|v| v.to_string()).collect(),
                    entropy_start: v.entropy_start,
                    entropy_end: v.entropy_end,
                })
            })
            .collect::<Result<_, String>>()?,
        native_entropy_words: words.iter().map(|v| v.to_string()).collect(),
    })
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct Initialization {
    ordinal: usize,
    base: BaseView,
    after_basis: BasisView,
    header: Header,
    current_facts: Facts,
    checkpoint_ordinal: usize,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct ObjectChange {
    card: String,
    before: CardObservation,
    after: CardObservation,
}
#[derive(Debug, Clone)]
pub(super) struct Accepted {
    pub ordinal: usize,
    pub policy_entry_ref: String,
    base: BaseView,
    input: InputView,
    after_basis: BasisView,
    header: Header,
    current_facts: Facts,
    card_changes: Vec<ObjectChange>,
    pile_changes: Vec<OrderedPileAfter>,
    token_delta: TokenDeltaEpoch,
}
#[derive(Debug, Clone)]
enum TokenDeltaEpoch {
    Absent,
    Mandatory(Vec<TokenChange>),
    OptionalPayment(Vec<TokenChange>),
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
// Diagnostic endpoint evidence owns complete before/after appearances. No runtime
// profile justifies changing this ownership solely for an enum-size heuristic.
#[allow(clippy::large_enum_variant)]
enum TokenChange {
    Added {
        object: String,
        after: TokenObservation,
    },
    Updated {
        object: String,
        before: TokenObservation,
        after: TokenObservation,
    },
    Removed {
        object: String,
        before: TokenObservation,
    },
}
fn token_changes(before: &World, after: &World) -> Vec<TokenChange> {
    let mut changes = Vec::new();
    let ids: std::collections::BTreeSet<_> =
        before.tokens.keys().chain(after.tokens.keys()).collect();
    for object in ids {
        match (before.tokens.get(object), after.tokens.get(object)) {
            (None, Some(new)) => changes.push(TokenChange::Added {
                object: object.clone(),
                after: new.clone(),
            }),
            (Some(old), None) => changes.push(TokenChange::Removed {
                object: object.clone(),
                before: old.clone(),
            }),
            (Some(old), Some(new)) if old != new => changes.push(TokenChange::Updated {
                object: object.clone(),
                before: old.clone(),
                after: new.clone(),
            }),
            _ => {}
        }
    }
    changes
}
impl Serialize for Accepted {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut row = serializer.serialize_struct(
            "Accepted",
            match &self.token_delta {
                TokenDeltaEpoch::Absent => 9,
                TokenDeltaEpoch::Mandatory(_) | TokenDeltaEpoch::OptionalPayment(_) => 10,
            },
        )?;
        macro_rules! fields { ($($name:ident),* $(,)?) => { $(row.serialize_field(stringify!($name), &self.$name)?;)* }; }
        fields!(
            ordinal,
            policy_entry_ref,
            base,
            input,
            after_basis,
            header,
            current_facts,
            card_changes,
            pile_changes
        );
        if let TokenDeltaEpoch::Mandatory(changes) | TokenDeltaEpoch::OptionalPayment(changes) =
            &self.token_delta
        {
            row.serialize_field("token_changes", changes)?;
        }
        row.end()
    }
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct FullCheckpoint {
    ordinal: usize,
    header_ref: usize,
    #[serde(flatten)]
    world: World,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct ErrorView {
    path: Vec<String>,
    payload: Option<serde_json::Value>,
}
fn error_view(value: &GameError) -> Result<ErrorView, String> {
    let canonical = crate::projection::game_error(value);
    let path = serde_json::from_value(canonical["error_path"].clone())
        .map_err(|error| format!("canonical rejection path: {error}"))?;
    let payload = canonical["error_leaf"]["data"].clone();
    Ok(ErrorView {
        path,
        payload: (!payload.is_null()).then_some(payload),
    })
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct Rejection {
    probe_id: String,
    expectation_ref: String,
    attachment: String,
    attached_candidate_ordinal: usize,
    original_basis: BasisView,
    attempted_input: InputView,
    expected_error: ErrorView,
    actual_error: ErrorView,
    no_candidate: bool,
    state_retained: bool,
    continuation_ordinal: Option<usize>,
}
#[derive(Debug, Clone)]
struct SavedProbe {
    ordinal: usize,
    id: String,
    input: GameInput,
    expected: GameError,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct RerunReport {
    initialization_equal: bool,
    compared_ordinals: Vec<usize>,
    comparison_fields: Vec<&'static str>,
    checkpoint_ordinals_compared: Vec<usize>,
    probe_ids_compared: Vec<String>,
    final_state_equal: bool,
    all_inputs_consumed: bool,
}
#[derive(Debug, Clone, Serialize)]
// Keep historical natural output intact. The bounded shape has an explicit kind
// and no outcome; this Serialize-only enum does not decode external input.
#[serde(untagged)]
pub(super) enum Completion {
    Natural(NaturalCompletion),
    Bounded(BoundedCompletion),
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct BoundedCompletion {
    kind: &'static str,
    boundary_ref: &'static str,
    position: crate::damage_trace::TurnPositionView,
    active: u32,
    holder: u32,
    final_ordinal: usize,
    checkpoint_ref: usize,
    accepted_inputs: usize,
    policy_entries_consumed: usize,
    policy_entries_total: usize,
    policy_complete: bool,
}
#[derive(Debug, Clone, Serialize)]
pub(super) struct NaturalCompletion {
    outcome: OutcomeView,
    final_ordinal: usize,
    checkpoint_ref: usize,
    entropy_position: Option<usize>,
    entropy_remaining: Option<usize>,
    accepted_inputs: usize,
    policy_entries_consumed: usize,
    policy_entries_total: usize,
    policy_complete: bool,
}

/// Both successful and failed attempts retain their available actual evidence.
#[derive(Debug, Serialize)]
pub struct RunReport {
    /// Private full native return/delivery history; old report bytes are unchanged.
    #[serde(skip)]
    native_trace: Option<NativeTrace>,
    schema_version: u32,
    fixture_ref: String,
    source_build_ref: &'static str,
    completed: bool,
    failure: Option<String>,
    static_inputs: Option<StaticInputs>,
    initialization: Option<Initialization>,
    accepted: Vec<Accepted>,
    checkpoints: BTreeMap<usize, FullCheckpoint>,
    observations: Vec<Observation>,
    rejection_probes: Vec<Rejection>,
    rerun: Option<RerunReport>,
    #[serde(rename = "final")]
    completion: Option<Completion>,
}
impl RunReport {
    pub fn completed(&self) -> bool {
        self.completed
    }
    pub fn accepted_inputs(&self) -> usize {
        self.accepted.len()
    }
    fn unsupported_selection(case: CaseId) -> Self {
        let mut report = Self::new(case);
        report.failure =
            Some("selection Report5 execution is unsupported in fixture-only admission".into());
        report
    }
    fn new(case: CaseId) -> Self {
        Self {
            native_trace: None,
            schema_version: match case.observation_contract().report_epoch {
                ReportEpoch::Report1 => 1,
                ReportEpoch::Report2 => 2,
                ReportEpoch::Report3 => 3,
                ReportEpoch::Report4 => 4,
                ReportEpoch::Report5 => 5,
            },
            fixture_ref: case.as_str().into(),
            source_build_ref: "native-execution.json",
            completed: false,
            failure: None,
            static_inputs: None,
            initialization: None,
            accepted: vec![],
            checkpoints: BTreeMap::new(),
            observations: vec![],
            rejection_probes: vec![],
            rerun: None,
            completion: None,
        }
    }
}
struct Recorder {
    report: RunReport,
    capture_prefix: String,
    history: History,
    world: Option<World>,
    selectors: Selectors,
    saved_probes: Vec<SavedProbe>,
    /// Actual transition-local appends, kept outside the frozen predecessor wire format.
    rules_journals: Vec<RulesJournal>,
}
impl Recorder {
    fn new(case: CaseId) -> Self {
        Self {
            report: RunReport::new(case),
            capture_prefix: "/observations/data/runs/0".into(),
            history: History::default(),
            world: None,
            selectors: Selectors::default(),
            saved_probes: vec![],
            rules_journals: vec![],
        }
    }
    fn capture(
        &self,
        fixture: &Fixture,
        state: &GameState,
        suffix: &str,
        owner_source: &str,
    ) -> Result<(), String> {
        if !crate::current_capture::is_active() {
            return Ok(());
        }
        let aliases = crate::packet::AliasContext::from_card_aliases(
            &fixture
                .aliases
                .iter()
                .map(|(id, row)| (*id, row.alias.clone()))
                .collect::<Vec<_>>(),
        )?;
        let point = crate::current_capture::observation(
            owner_source,
            crate::current_profile::SnapshotOwner::GameState(state),
            &aliases,
        )?;
        crate::current_capture::bind_observation(
            &point,
            &format!("{}{}", self.capture_prefix, suffix),
        )
    }

    fn initial(&mut self, fixture: &Fixture, candidate: &NativeCandidate) -> Result<(), String> {
        self.capture(
            fixture,
            candidate.state(),
            "/initialization",
            "record::Recorder::initial",
        )?;
        self.history.remember(None, candidate);
        let world = view::world(fixture, &self.history, candidate.state())?;
        self.report.initialization = Some(Initialization {
            ordinal: 0,
            base: view::base(fixture, candidate.base())?,
            after_basis: view::basis(fixture, candidate.state().basis())?,
            header: view::header(fixture, &self.history, candidate.state())?,
            current_facts: view::facts(fixture, &self.history, None, candidate)?,
            checkpoint_ordinal: 0,
        });
        self.names(fixture, None, None, candidate, 0, &world)?;
        self.world = Some(world);
        self.rules_journals.push(candidate.rules_journal().clone());
        Ok(())
    }
    fn names(
        &mut self,
        fixture: &Fixture,
        source: Option<&GameState>,
        input: Option<&GameInput>,
        candidate: &NativeCandidate,
        ordinal: usize,
        world: &World,
    ) -> Result<(), String> {
        let names = self
            .selectors
            .observe(fixture, source, input, candidate, ordinal)?;
        if names.iter().any(|v| v.scope == Scope::FullCheckpoint) {
            self.capture(
                fixture,
                candidate.state(),
                &format!("/checkpoints/{ordinal}"),
                "record::Recorder::names full checkpoint",
            )?;
            self.report.checkpoints.insert(
                ordinal,
                FullCheckpoint {
                    ordinal,
                    header_ref: ordinal,
                    world: world.clone(),
                },
            );
        }
        for index in self.report.observations.len()..self.report.observations.len() + names.len() {
            self.capture(
                fixture,
                candidate.state(),
                &format!("/observations/{index}"),
                "record::Recorder::names actual selector",
            )?;
        }
        self.report.observations.extend(names);
        Ok(())
    }
    fn accepted(
        &mut self,
        fixture: &Fixture,
        source: &GameState,
        input: &GameInput,
        candidate: &NativeCandidate,
        policy_ref: String,
    ) -> Result<(), String> {
        self.capture(
            fixture,
            candidate.state(),
            &format!("/accepted/{}", self.report.accepted.len()),
            "record::Recorder::accepted",
        )?;
        self.history.remember(Some(source), candidate);
        let next_world = view::world(fixture, &self.history, candidate.state())?;
        let ordinal = self.report.accepted.len() + 1;
        let record = accepted_record(
            TransitionObservation {
                fixture,
                history: &self.history,
                source,
                input,
                candidate,
                before: self.world.as_ref().ok_or("recorder lacks initial world")?,
                after: &next_world,
            },
            ordinal,
            policy_ref,
        )?;
        self.names(
            fixture,
            Some(source),
            Some(input),
            candidate,
            ordinal,
            &next_world,
        )?;
        self.report.accepted.push(record);
        self.world = Some(next_world);
        self.rules_journals.push(candidate.rules_journal().clone());
        Ok(())
    }
    fn probes(
        &mut self,
        fixture: &Fixture,
        policy: &Policy,
        state: &GameState,
        prepared: Option<&PreparedInput>,
    ) -> Result<(), String> {
        let selected = probes::reached_probes(fixture, policy, state, prepared)?;
        if selected.is_empty() {
            return Ok(());
        }
        if let Some(limit) = policy.reached_probe_limit() {
            let total = self
                .saved_probes
                .len()
                .checked_add(selected.len())
                .and_then(|total| u64::try_from(total).ok())
                .ok_or("reached rejection probe count overflow")?;
            if total > limit {
                return Err("native fixture reached rejection probe budget exhausted".into());
            }
        }
        let ordinal = self.report.accepted.len();
        let original = state.clone();
        for probe in selected {
            let id = format!("{}@{ordinal}", probe.id);
            let attempted = view::input(fixture, &self.history, &probe.input)?;
            let returned = if staged::finalized_capture() {
                staged::probe(fixture, self, state, probe.input.clone())?
            } else {
                crate::current_capture::advance(state, probe.input.clone())
            };
            let actual = match returned {
                Err(error) => error,
                Ok(_) => {
                    return Err(format!(
                        "rejection probe {id} unexpectedly returned a native candidate"
                    ));
                }
            };
            let retained = state == &original;
            self.capture(
                fixture,
                state,
                &format!("/rejection_probes/{}", self.report.rejection_probes.len()),
                "record::Recorder::probes actual reached rejection",
            )?;
            self.report.rejection_probes.push(Rejection {
                probe_id: id.clone(),
                expectation_ref: probe.expectation_ref,
                attachment: probe.attachment,
                attached_candidate_ordinal: ordinal,
                original_basis: view::basis(fixture, state.basis())?,
                attempted_input: attempted,
                expected_error: error_view(&probe.expected)?,
                actual_error: error_view(&actual)?,
                no_candidate: true,
                state_retained: retained,
                continuation_ordinal: if state.outcome().is_some() || prepared.is_none() {
                    None
                } else {
                    Some(ordinal + 1)
                },
            });
            if actual != probe.expected || !retained {
                return Err(format!(
                    "rejection probe {id} mismatch: expected {:?}, actual {actual:?}, state_retained={retained}",
                    probe.expected
                ));
            }
            self.saved_probes.push(SavedProbe {
                ordinal,
                id,
                input: probe.input,
                expected: actual,
            });
        }
        Ok(())
    }
}
struct TransitionObservation<'a> {
    fixture: &'a Fixture,
    history: &'a History,
    source: &'a GameState,
    input: &'a GameInput,
    candidate: &'a NativeCandidate,
    before: &'a World,
    after: &'a World,
}
fn accepted_record(
    observed: TransitionObservation<'_>,
    ordinal: usize,
    policy_entry_ref: String,
) -> Result<Accepted, String> {
    let TransitionObservation {
        fixture,
        history,
        source,
        input,
        candidate,
        before,
        after,
    } = observed;
    if &input.basis != source.basis() {
        return Err("accepted observation input differs from actual source basis".into());
    }
    let mut card_changes = vec![];
    if before.cards.keys().collect::<Vec<_>>() != after.cards.keys().collect::<Vec<_>>() {
        return Err("physical-card domain changed in closed fixture".into());
    }
    for (id, old) in &before.cards {
        let new = after.cards.get(id).ok_or("missing observed card")?;
        if old != new {
            card_changes.push(ObjectChange {
                card: id.clone(),
                before: old.clone(),
                after: new.clone(),
            });
        }
    }
    let mut pile_changes = vec![];
    if before.ordered_piles.len() != after.ordered_piles.len() {
        return Err("ordered pile domain changed".into());
    }
    for (old, new) in before.ordered_piles.iter().zip(&after.ordered_piles) {
        if old.pile != new.pile {
            return Err("ordered pile key changed".into());
        }
        if old != new {
            pile_changes.push(new.clone());
        }
    }
    Ok(Accepted {
        ordinal,
        policy_entry_ref,
        base: view::base(fixture, candidate.base())?,
        input: view::input(fixture, history, input)?,
        after_basis: view::basis(fixture, candidate.state().basis())?,
        header: view::header(fixture, history, candidate.state())?,
        current_facts: view::facts(fixture, history, Some(source), candidate)?,
        card_changes,
        pile_changes,
        token_delta: match fixture.case.observation_contract().report_epoch {
            ReportEpoch::Report1 | ReportEpoch::Report2 => TokenDeltaEpoch::Absent,
            ReportEpoch::Report3 => TokenDeltaEpoch::Mandatory(token_changes(before, after)),
            ReportEpoch::Report4 => TokenDeltaEpoch::OptionalPayment(token_changes(before, after)),
            ReportEpoch::Report5 => {
                return Err("selection Report5 accepted projection is unsupported".into());
            }
        },
    })
}
fn rerun(
    fixture: &Fixture,
    report: &RunReport,
    inputs: &[GameInput],
    saved_probes: &[SavedProbe],
    rules_journals: &[RulesJournal],
    final_state: &GameState,
) -> Result<RerunReport, String> {
    if report.accepted.len() != inputs.len() || rules_journals.len() != inputs.len() + 1 {
        return Err(
            "rerun requires one rules journal for initialization and each saved input".into(),
        );
    }
    let initial = crate::current_capture::without_capture(|| initialize_fixture(fixture))
        .map_err(|e| format!("rerun initialization: {e:?}"))?;
    let mut history = History::default();
    history.remember(None, &initial);
    let mut world = view::world(fixture, &history, initial.state())?;
    let observed = Initialization {
        ordinal: 0,
        base: view::base(fixture, initial.base())?,
        after_basis: view::basis(fixture, initial.state().basis())?,
        header: view::header(fixture, &history, initial.state())?,
        current_facts: view::facts(fixture, &history, None, &initial)?,
        checkpoint_ordinal: 0,
    };
    if !equal(
        &observed,
        report
            .initialization
            .as_ref()
            .ok_or("first initialization missing")?,
    )? {
        return Err("rerun initialization observation differs".into());
    }
    compare_rules_journal(&rules_journals[0], &initial, 0)?;
    let mut checkpoints = vec![];
    let mut compared = vec![];
    let mut compared_probes = vec![];
    let mut state = initial.into_state();
    for ordinal in 0..=inputs.len() {
        if let Some(checkpoint) = report.checkpoints.get(&ordinal) {
            if !equal(
                &FullCheckpoint {
                    ordinal,
                    header_ref: ordinal,
                    world: world.clone(),
                },
                checkpoint,
            )? {
                return Err(format!("rerun full checkpoint differs at {ordinal}"));
            }
            checkpoints.push(ordinal);
        }
        let actual_probes = saved_probes
            .iter()
            .filter(|v| v.ordinal == ordinal)
            .collect::<Vec<_>>();
        if !actual_probes.is_empty() {
            let retained = state.clone();
            for probe in actual_probes {
                match state.advance(probe.input.clone()) {
                    Err(error) if error == probe.expected => {}
                    Err(error) => {
                        return Err(format!(
                            "rerun probe {} changed rejection: {error:?}",
                            probe.id
                        ));
                    }
                    Ok(_) => return Err(format!("rerun probe {} returned a candidate", probe.id)),
                }
                if state != retained {
                    return Err(format!("rerun probe {} mutated its source", probe.id));
                }
                compared_probes.push(probe.id.clone());
            }
        }
        let Some(input) = inputs.get(ordinal) else {
            break;
        };
        let candidate = state
            .advance(input.clone())
            .map_err(|e| format!("rerun input {} failed: {e:?}", ordinal + 1))?;
        history.remember(Some(&state), &candidate);
        let after = view::world(fixture, &history, candidate.state())?;
        let first = report
            .accepted
            .get(ordinal)
            .ok_or("missing first-run accepted input")?;
        let actual = accepted_record(
            TransitionObservation {
                fixture,
                history: &history,
                source: &state,
                input,
                candidate: &candidate,
                before: &world,
                after: &after,
            },
            ordinal + 1,
            first.policy_entry_ref.clone(),
        )?;
        if !equal(&actual, first)? {
            return Err(format!(
                "rerun actual candidate record differs at {}",
                ordinal + 1
            ));
        }
        compare_rules_journal(&rules_journals[ordinal + 1], &candidate, ordinal + 1)?;
        compared.push(ordinal + 1);
        world = after;
        state = candidate.into_state();
    }
    if &state != final_state {
        return Err("rerun full final GameState equality failed".into());
    }
    let mut comparison_fields = vec![
        "actual_candidate_base",
        "retained_actual_input",
        "actual_successor_basis",
        "header",
        "all_twelve_current_fact_families",
        "complete_observed_card_changes",
        "changed_ordered_pile_after_vectors",
    ];
    if fixture.case.observation_contract().report_epoch == ReportEpoch::Report2 {
        comparison_fields[4] = "twelve_predecessor_current_fact_families";
        comparison_fields.extend([
            "interaction_execution_stack_and_resolution",
            "interaction_instruction_journal",
            "complete_native_rules_journals",
            "full_final_native_state",
        ]);
    }
    if fixture.case.observation_contract().report_epoch == ReportEpoch::Report3 {
        comparison_fields[4] = "twelve_predecessor_current_fact_families";
        comparison_fields.extend([
            "canonical_execution_stack_resolution_waiting_and_pending_placement",
            "complete_token_observations_and_endpoint_deltas",
            "canonical_rules_journal",
            "complete_native_rules_journals",
            "full_final_native_state",
        ]);
    }
    if fixture.case.observation_contract().report_epoch == ReportEpoch::Report4 {
        comparison_fields[4] = "twelve_predecessor_current_fact_families";
        comparison_fields.extend([
            "canonical_execution_stack_resolution_waiting_and_pending_placement",
            "complete_pending_work_and_suspended_payment_parent",
            "complete_spell_copy_objects",
            "complete_token_observations_and_endpoint_deltas",
            "canonical_rules_journal_with_optional_and_payment_controls",
            "complete_native_rules_journals",
            "full_final_native_state",
        ]);
    }
    Ok(RerunReport {
        initialization_equal: true,
        compared_ordinals: compared,
        comparison_fields,
        checkpoint_ordinals_compared: checkpoints,
        probe_ids_compared: compared_probes,
        final_state_equal: true,
        all_inputs_consumed: true,
    })
}
/// Compare native appends directly; final state and predecessor facts cannot recover them.
fn compare_rules_journal(
    first: &RulesJournal,
    candidate: &NativeCandidate,
    ordinal: usize,
) -> Result<(), String> {
    if first != candidate.rules_journal() {
        return Err(format!("rerun complete rules journal differs at {ordinal}"));
    }
    Ok(())
}
fn execute(
    case: CaseId,
    recorder: &mut Recorder,
    vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Result<(), String> {
    let fixture = Fixture::new(case, vocabulary)?;
    execute_fixture(&fixture, recorder)
}
/// Use the same capture-aware native constructor for execution and reexecution.
/// Rerun disables only collection around this call, never the rules operation.
fn initialize_fixture(fixture: &Fixture) -> Result<NativeCandidate, GameError> {
    match fixture.initialization {
        FixtureInitialization::Opening => crate::current_capture::initialize_opening(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.clone(),
            fixture
                .shuffles
                .iter()
                .flat_map(|recipe| recipe.words.iter().copied())
                .collect(),
        ),
        FixtureInitialization::Scheduled(start) => crate::current_capture::initialize_scheduled(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.seed.clone(),
            start,
        ),
    }
}
fn execute_fixture(fixture: &Fixture, recorder: &mut Recorder) -> Result<(), String> {
    recorder.report.static_inputs = Some(static_inputs(fixture)?);
    if crate::current_capture::is_active() {
        let aliases = crate::packet::AliasContext::from_card_aliases(
            &fixture
                .aliases
                .iter()
                .map(|(id, row)| (*id, row.alias.clone()))
                .collect::<Vec<_>>(),
        )?;
        crate::current_capture::snapshot_at(
            &format!("ordinary-static-{}", recorder.capture_prefix),
            &[format!("{}/static_inputs", recorder.capture_prefix)],
            crate::current_profile::Phase::InitializeInput,
            "record::execute actual supplied fixture",
            crate::current_profile::SnapshotOwner::InitializeInput {
                context: Some(&fixture.context),
                seed: &fixture.setup.seed,
                source_ref: "compiled ordinary fixture inputs",
            },
            &aliases,
        )?;
    }
    let mut policy = Policy::new(fixture)?;
    let finalized = staged::finalized_capture();
    let mut state = if finalized {
        staged::initialize_and_install(fixture, recorder)?
    } else {
        let initial =
            initialize_fixture(fixture).map_err(|e| format!("initialization failed: {e:?}"))?;
        recorder.initial(fixture, &initial)?;
        initial.into_state()
    };
    // Keep the accepted native state alive across every later caller failure.
    // The closure captures policy/observer errors without bypassing final replay.
    let execution = (|| -> Result<(), String> {
        let mut inputs = vec![];
        let stop = loop {
            let prepared = policy.prepare(&state)?;
            recorder.probes(fixture, &policy, &state, prepared.as_ref())?;
            if state.outcome().is_some() {
                break None;
            }
            if let Some(boundary) = policy.completed_boundary(&state)? {
                break Some(boundary);
            }
            if inputs.len() >= 4096 {
                return Err(
                    "native fixture command watchdog exhausted before required completion".into(),
                );
            }
            let prepared = prepared.ok_or("nonterminal policy has no prepared input")?;
            let policy_ref = prepared.policy_entry_ref.clone();
            let input = policy.issue(&state, prepared)?;
            if finalized {
                staged::advance_and_install(
                    fixture,
                    recorder,
                    &mut policy,
                    &mut state,
                    input.clone(),
                    policy_ref,
                )?;
                inputs.push(input);
            } else {
                let candidate =
                    crate::current_capture::advance(&state, input.clone()).map_err(|e| {
                        format!(
                            "accepted policy input {} ({policy_ref}) failed: {e:?}",
                            inputs.len() + 1
                        )
                    })?;
                recorder.accepted(fixture, &state, &input, &candidate, policy_ref)?;
                policy.accept(&input, &candidate)?;
                inputs.push(input);
                state = candidate.into_state();
            }
        };
        for required in policy.required_recipe_probe_ids() {
            let count = recorder
                .saved_probes
                .iter()
                .filter(|probe| {
                    probe
                        .id
                        .rsplit_once('@')
                        .is_some_and(|(id, _)| id == required)
                })
                .count();
            if count != 1 {
                return Err(format!(
                    "required recipe probe {required} executed {count} times"
                ));
            }
        }
        policy.finish(&state)?;
        recorder.selectors.finish(fixture.case)?;
        if !recorder.report.checkpoints.contains_key(&inputs.len()) {
            recorder.capture(
                fixture,
                &state,
                &format!("/checkpoints/{}", inputs.len()),
                "record::execute actual final checkpoint",
            )?;
            recorder.report.checkpoints.insert(
                inputs.len(),
                FullCheckpoint {
                    ordinal: inputs.len(),
                    header_ref: inputs.len(),
                    world: recorder.world.clone().ok_or("final actual world absent")?,
                },
            );
        }
        recorder.report.rerun = Some(rerun(
            fixture,
            &recorder.report,
            &inputs,
            &recorder.saved_probes,
            &recorder.rules_journals,
            &state,
        )?);
        recorder.report.completion = Some(match stop {
            Some(StopKind::PreparedReadyPriority {
                boundary_ref,
                position,
                active,
                holder,
            }) => Completion::Bounded(BoundedCompletion {
                kind: "bounded_prefix",
                boundary_ref,
                position: crate::damage_trace::turn_position_view(position),
                active: active.0,
                holder: holder.0,
                final_ordinal: inputs.len(),
                checkpoint_ref: inputs.len(),
                accepted_inputs: inputs.len(),
                policy_entries_consumed: policy.consumed(),
                policy_entries_total: policy.script_len(),
                policy_complete: true,
            }),
            None => Completion::Natural(NaturalCompletion {
                outcome: view::outcome(state.outcome().ok_or("terminal outcome absent")?),
                final_ordinal: inputs.len(),
                checkpoint_ref: inputs.len(),
                entropy_position: state.entropy().map(|entropy| entropy.position()),
                entropy_remaining: state.entropy().map(|entropy| entropy.remaining()),
                accepted_inputs: inputs.len(),
                policy_entries_consumed: policy.consumed(),
                policy_entries_total: policy.script_len(),
                policy_complete: true,
            }),
        });
        recorder.capture(
            fixture,
            &state,
            "/final",
            "record::execute actual final GameState",
        )?;
        Ok(())
    })();
    finalize_execution(fixture, recorder, &state, finalized, execution)
}

fn finalize_execution(
    fixture: &Fixture,
    recorder: &mut Recorder,
    state: &GameState,
    finalized: bool,
    execution: Result<(), String>,
) -> Result<(), String> {
    recorder.report.completed = false;
    if finalized && let Err(error) = staged::verify_trace(fixture, recorder, Some(state)) {
        // A replay/observer failure cannot overwrite an original caller error or
        // native Result. It independently prevents successful qualification.
        crate::current_capture::retain_observer_failure(error.clone());
        return execution.and(Err(error));
    }
    execution?;
    recorder.report.completed = true;
    Ok(())
}

pub(super) fn run_fixture_at(fixture: Fixture, index: usize) -> RunReport {
    if matches!(fixture.case, CaseId::Selection(_)) {
        return RunReport::unsupported_selection(fixture.case);
    }
    let mut recorder = Recorder::new(fixture.case);
    recorder.capture_prefix = format!("/observations/data/runs/{index}");
    if let Err(error) = execute_fixture(&fixture, &mut recorder) {
        recorder.report.failure = Some(error);
    }
    recorder.report
}
/// Run only compiled source-backed fixture inputs; preserve partial failed runs.
pub fn run_case(
    case: CaseId,
    vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> RunReport {
    run_case_at(case, 0, vocabulary)
}
pub(super) fn run_case_at(
    case: CaseId,
    index: usize,
    vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> RunReport {
    if matches!(case, CaseId::Selection(_)) {
        return RunReport::unsupported_selection(case);
    }
    let mut recorder = Recorder::new(case);
    recorder.capture_prefix = format!("/observations/data/runs/{index}");
    if let Err(error) = execute(case, &mut recorder, vocabulary) {
        recorder.report.failure = Some(error);
    }
    recorder.report
}

#[cfg(test)]
pub(in crate::ordinary_game) fn static_input_bytes_for_test(
    fixture: &Fixture,
) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&static_inputs(fixture)?)
        .map_err(|error| format!("static input serialization: {error}"))
}

#[cfg(test)]
mod selection_cap_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exiled_seed_recording_is_scoped_to_scheduled_report4_inputs() {
        let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
        let mut fixture = Fixture::new(CaseId::G01, vocabulary).unwrap();
        let mut card = fixture.setup.seed.cards[0].clone();
        card.destination = ObjectDestination::Exile;
        fixture.initialization =
            FixtureInitialization::Scheduled(spellwake_engine::turn::TurnStart {
                number: 3,
                step: spellwake_engine::turn::Step::PrecombatMain,
            });
        for case in [
            CaseId::G01,
            CaseId::Interaction(super::super::interaction::InteractionCase::Bi01),
            CaseId::Mandatory(super::super::mandatory::MandatoryCase::ALL[0]),
        ] {
            fixture.case = case;
            assert!(seed_view(&fixture, &card).is_err());
        }
        fixture.case = CaseId::Optional(super::super::optional::OptionalCase::R02);
        let recorded = serde_json::to_value(seed_view(&fixture, &card).unwrap()).unwrap();
        assert_eq!(recorded["destination"], "exile");
        assert_eq!(recorded["card_id"], card.card.0.to_string());
        fixture.initialization = FixtureInitialization::Opening;
        assert!(seed_view(&fixture, &card).is_err());
    }
    use spellwake_engine::occurrence::{EventId, RulesRecord};

    struct OpeningPrefix {
        fixture: Fixture,
        recorder: Recorder,
        inputs: Vec<GameInput>,
        state: GameState,
    }
    impl OpeningPrefix {
        fn capture() -> Self {
            let fixture = Fixture::new(
                CaseId::G01,
                crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit"),
            )
            .unwrap();
            let mut recorder = Recorder::new(fixture.case);
            let mut policy = Policy::new(&fixture).unwrap();
            let initial = GameState::initialize_opening(
                fixture.game,
                fixture.context.clone(),
                fixture.setup.clone(),
            )
            .unwrap();
            recorder.initial(&fixture, &initial).unwrap();
            let mut state = initial.into_state();
            let mut inputs = Vec::new();
            // Two actual keep replies hand off to the first ordinary turn.
            for _ in 0..2 {
                let prepared = policy.prepare(&state).unwrap().unwrap();
                let policy_ref = prepared.policy_entry_ref.clone();
                let input = policy.issue(&state, prepared).unwrap();
                let candidate = state.advance(input.clone()).unwrap();
                recorder
                    .accepted(&fixture, &state, &input, &candidate, policy_ref)
                    .unwrap();
                policy.accept(&input, &candidate).unwrap();
                inputs.push(input);
                state = candidate.into_state();
            }
            Self {
                fixture,
                recorder,
                inputs,
                state,
            }
        }
        fn rerun(&self, rules_journals: &[RulesJournal]) -> Result<RerunReport, String> {
            rerun(
                &self.fixture,
                &self.recorder.report,
                &self.inputs,
                &self.recorder.saved_probes,
                rules_journals,
                &self.state,
            )
        }
    }

    #[test]
    fn ordinary_rules_journal_receipts_preserve_initialization_and_each_saved_input() {
        let run = OpeningPrefix::capture();
        assert_eq!(run.recorder.rules_journals.len(), run.inputs.len() + 1);
        let report = run.rerun(&run.recorder.rules_journals).unwrap();
        assert!(report.initialization_equal);
        assert_eq!(report.compared_ordinals, vec![1, 2]);
        assert!(report.final_state_equal);
        assert!(report.all_inputs_consumed);
        let old_packet = serde_json::to_value(&run.recorder.report).unwrap();
        assert_eq!(
            old_packet["initialization"]["current_facts"]
                .as_object()
                .unwrap()
                .len(),
            12
        );
        assert!(old_packet.get("rules_journals").is_none());
        for accepted in old_packet["accepted"].as_array().unwrap() {
            assert_eq!(accepted["current_facts"].as_object().unwrap().len(), 12);
        }
        for malformed in [
            &run.recorder.rules_journals[..2],
            &[
                run.recorder.rules_journals.clone(),
                vec![RulesJournal::default()],
            ]
            .concat(),
        ] {
            assert_eq!(
                run.rerun(malformed).unwrap_err(),
                "rerun requires one rules journal for initialization and each saved input"
            );
        }
        let mut reordered = run.recorder.rules_journals.clone();
        reordered.swap(0, 1);
        assert_eq!(
            run.rerun(&reordered).unwrap_err(),
            "rerun complete rules journal differs at 0"
        );
    }

    fn reject_append_only_corruption(mutate: fn(&mut RulesJournal, EventId)) {
        let run = OpeningPrefix::capture();
        let old_packet = serde_json::to_value(&run.recorder.report).unwrap();
        let final_state = run.state.clone();
        let preserved = run.rerun(&run.recorder.rules_journals).unwrap();
        assert!(preserved.final_state_equal);
        let accepted_ordinal = run
            .recorder
            .rules_journals
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, journal)| !journal.records.is_empty())
            .map(|(ordinal, _)| ordinal)
            .expect("opening handoff has an actual accepted-candidate append");
        // Exercise the initialization comparison and the accepted-candidate comparison.
        for ordinal in [0, accepted_ordinal] {
            let original = &run.recorder.rules_journals[ordinal];
            let first_event = original
                .records
                .iter()
                .find_map(|record| match record {
                    RulesRecord::Event(event) => Some(event.id),
                    _ => None,
                })
                .unwrap();
            let other_event = run
                .recorder
                .rules_journals
                .iter()
                .flat_map(|journal| &journal.records)
                .find_map(|record| match record {
                    RulesRecord::Event(event) if event.id != first_event => Some(event.id),
                    _ => None,
                })
                .expect("another actual event supplies a different binding");
            let mut corrupted = run.recorder.rules_journals.clone();
            mutate(&mut corrupted[ordinal], other_event);
            assert_ne!(corrupted[ordinal], *original);
            assert_eq!(
                serde_json::to_value(&run.recorder.report).unwrap(),
                old_packet,
                "append-only corruption leaves all predecessor observations unchanged"
            );
            assert_eq!(run.state, final_state);
            assert_eq!(
                run.rerun(&corrupted).unwrap_err(),
                format!("rerun complete rules journal differs at {ordinal}")
            );
        }
    }

    #[test]
    fn ordinary_reexecution_rejects_append_only_deletion() {
        reject_append_only_corruption(|journal, _| {
            journal.records.remove(0);
        });
    }

    #[test]
    fn ordinary_reexecution_rejects_append_only_duplication() {
        reject_append_only_corruption(|journal, _| {
            journal.records.insert(0, journal.records[0].clone());
        });
    }

    #[test]
    fn ordinary_reexecution_rejects_append_only_occurrence_rebinding() {
        reject_append_only_corruption(|journal, other_event| {
            let occurrence = journal
                .records
                .iter_mut()
                .find_map(|record| match record {
                    RulesRecord::Event(event) => event.occurrences.first_mut(),
                    _ => None,
                })
                .unwrap();
            occurrence.id.event = other_event;
        });
    }
}
