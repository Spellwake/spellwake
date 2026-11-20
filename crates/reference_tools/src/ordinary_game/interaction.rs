//! Finite source-backed Blue interaction inputs over the shared ordinary runner.
//! Recipes supply commands and preconditions. Only GameState decides their results.
use super::{
    catalogue::{Deck, DefinitionInput, InventoryEntry, Materialized},
    fixture::{A, B, CaseId, ContextInputs, Fixture, FixtureInitialization, other},
    policy::{Policy, StopKind},
    probes::RejectionProbe,
    record::{self, RunReport},
};
use serde::{Deserialize, Serialize};
use spellwake_card_programs::{
    INTERACTION_PROFILE, INTERACTION_REGISTRY_VERSION, Registry, TEACHING_SOURCE_SHA256,
};
use spellwake_content::Inventory;
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeVocabulary};
use spellwake_engine::{
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision, CombatRules},
    game::{
        DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate,
        OpeningSetup, RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaPayment, ManaSpend, ManaType},
    object_identity::{ObjectDestination, ObjectId, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord},
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    shuffle::EntropyTape,
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

/// Closed finite set: two complete games and thirteen prepared mechanism cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionCase {
    Bi01,
    Bi02,
    If01,
    If02,
    If03,
    If04,
    If05,
    If06,
    If07,
    If08,
    If09,
    If10,
    If11,
    If12,
    If15,
}
impl InteractionCase {
    pub const FULL_GAMES: [Self; 2] = [Self::Bi01, Self::Bi02];
    pub const FOCUSED: [Self; 13] = [
        Self::If01,
        Self::If02,
        Self::If03,
        Self::If04,
        Self::If05,
        Self::If06,
        Self::If07,
        Self::If08,
        Self::If09,
        Self::If10,
        Self::If11,
        Self::If12,
        Self::If15,
    ];
    pub const ALL: [Self; 15] = [
        Self::Bi01,
        Self::Bi02,
        Self::If01,
        Self::If02,
        Self::If03,
        Self::If04,
        Self::If05,
        Self::If06,
        Self::If07,
        Self::If08,
        Self::If09,
        Self::If10,
        Self::If11,
        Self::If12,
        Self::If15,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bi01 => "BI01",
            Self::Bi02 => "BI02",
            Self::If01 => "IF01",
            Self::If02 => "IF02",
            Self::If03 => "IF03",
            Self::If04 => "IF04",
            Self::If05 => "IF05",
            Self::If06 => "IF06",
            Self::If07 => "IF07",
            Self::If08 => "IF08",
            Self::If09 => "IF09",
            Self::If10 => "IF10",
            Self::If11 => "IF11",
            Self::If12 => "IF12",
            Self::If15 => "IF15",
        }
    }
    pub(super) const fn game_id(self) -> u128 {
        match self {
            Self::Bi01 => 5301,
            Self::Bi02 => 5302,
            Self::If01 => 5401,
            Self::If02 => 5402,
            Self::If03 => 5403,
            Self::If04 => 5404,
            Self::If05 => 5405,
            Self::If06 => 5406,
            Self::If07 => 5407,
            Self::If08 => 5408,
            Self::If09 => 5409,
            Self::If10 => 5410,
            Self::If11 => 5411,
            Self::If12 => 5412,
            Self::If15 => 5415,
        }
    }
    /// Reject unknown identifiers, including IF13/IF14 which name suites, not runs.
    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|case| case.as_str() == value)
            .ok_or_else(|| format!("unknown interaction case {value}"))
    }
    pub(super) const fn boundary_ref(self) -> &'static str {
        match self {
            Self::If01 => "IF01.settled",
            Self::If02 => "IF02.settled",
            Self::If03 => "IF03.settled",
            Self::If04 => "IF04.settled",
            Self::If05 => "IF05.settled",
            Self::If06 => "IF06.settled",
            Self::If07 => "IF07.settled",
            Self::If08 => "IF08.settled",
            Self::If09 => "IF09.settled",
            Self::If10 => "IF10.settled",
            Self::If15 => "IF15.settled",
            _ => "natural",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    source_recipe_hashes: BTreeMap<String, String>,
    inventory: BTreeMap<String, InventoryEntry>,
    decks: Vec<Deck>,
    definitions: Vec<DefinitionInput>,
    cases: Vec<CaseInput>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseInput {
    id: String,
    starting_role: String,
    opening_declarations: Vec<Declaration>,
    shuffle_target_orders_top_first: BTreeMap<String, Vec<String>>,
    prepared: Option<Prepared>,
    actions: Vec<Action>,
    combat: Vec<Combat>,
    stop: Stop,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    role: String,
    choice: String,
    taken: u8,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    active_role: String,
    individual_turn: u64,
    step: String,
    initial_step: String,
    battlefield: BTreeMap<String, Vec<String>>,
    hand: BTreeMap<String, Vec<String>>,
    library_top_first: BTreeMap<String, Vec<String>>,
    graveyard_top_first: BTreeMap<String, Vec<String>>,
    initial_battlefield_order: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Stop {
    NaturalOutcome,
    ReadyPriority {
        individual_turn: u64,
        step: String,
        active_role: String,
        priority_holder: String,
        stack: Vec<String>,
        all_actions_consumed: bool,
        required_settlements: Vec<String>,
        boundary_ref: String,
    },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ActionKind {
    PlayLand,
    ActivateIntrinsicMana,
    Cast,
    PassPriority,
    DeclareAttackers,
    DeclareBlockers,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Object { card: String },
    Stack { spell: String },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    id: String,
    actor_role: String,
    active_role: String,
    individual_turn: u64,
    step: String,
    kind: ActionKind,
    card: Option<String>,
    target: Option<Target>,
    payment: BTreeMap<String, String>,
    expected_stack_bottom_first: Vec<String>,
    expected_stack_cast_actions_bottom_first: Vec<String>,
    target_cast_action: Option<String>,
    after_settled_action: Option<String>,
    attackers: Vec<String>,
    blockers: Vec<Block>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Combat {
    active_role: String,
    own_turn: u64,
    expected_eligible_attackers: Vec<String>,
    attackers: Vec<String>,
    blockers: Vec<Block>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    blocker: String,
    attacker: String,
}
fn inputs() -> Result<Inputs, String> {
    let value: Inputs = serde_json::from_str(include_str!("interaction-inputs-v1.json"))
        .map_err(|e| format!("invalid compiled interaction inputs: {e}"))?;
    for (path, hash) in [
        (
            "content/manifest.json",
            "80dcf249f55095ab1bcb4942e4ebf9710fc1c63cc822019ad419c4a07c789165",
        ),
        (
            "qualification/cases-v1.json",
            "828c3c353804c1f1b8ab789a33c29a7d615d9da5cbd90a92d2d1dc00f32eefcd",
        ),
        (
            "qualification/focused-cases-v3.json",
            "ebdce1ea1819b116f48532e79aedb038e828940d7981403c1ad70ee22e025045",
        ),
    ] {
        if value.source_recipe_hashes.get(path).map(String::as_str) != Some(hash) {
            return Err("interaction recipe authority mismatch".into());
        }
    }
    if value.source_recipe_hashes.len() != 3
        || value.cases.len() != 15
        || value
            .cases
            .iter()
            .map(|c| &c.id)
            .collect::<BTreeSet<_>>()
            .len()
            != 15
    {
        return Err("interaction closed case domain mismatch".into());
    }
    for c in &value.cases {
        InteractionCase::parse(&c.id)?;
        if c.actions
            .iter()
            .map(|a| &a.id)
            .collect::<BTreeSet<_>>()
            .len()
            != c.actions.len()
        {
            return Err("duplicate named interaction action".into());
        }
        for a in &c.actions {
            seat(&a.actor_role)?;
            seat(&a.active_role)?;
            step(&a.step)?;
        }
    }
    Ok(value)
}
fn seat(role: &str) -> Result<SeatId, String> {
    match role {
        "U" => Ok(A),
        "G" => Ok(B),
        _ => Err(format!("unknown interaction role {role}")),
    }
}
fn role(seat: SeatId) -> Result<&'static str, String> {
    match seat {
        A => Ok("U"),
        B => Ok("G"),
        _ => Err("unknown interaction seat".into()),
    }
}
fn step(value: &str) -> Result<Step, String> {
    match value {
        "untap" => Ok(Step::Untap),
        "upkeep" => Ok(Step::Upkeep),
        "draw" => Ok(Step::Draw),
        "precombat_main" => Ok(Step::PrecombatMain),
        "beginning_combat" => Ok(Step::BeginningCombat),
        "declare_attackers" => Ok(Step::DeclareAttackers),
        "declare_blockers" => Ok(Step::DeclareBlockers),
        "postcombat_main" => Ok(Step::PostcombatMain),
        _ => Err(format!("unknown interaction step {value}")),
    }
}

/// Actual hash-checked catalogue and the sole guarded interaction Registry.
pub struct InteractionSource {
    inventory: Inventory,
    registry: Registry,
    inputs: Inputs,
}
impl InteractionSource {
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    /// Load selected definitions from actual source-qualified inventory bytes.
    /// # Errors
    /// Rejects source/registry/vocabulary mismatch or malformed compiled inputs.
    pub fn load(
        metadata: &Path,
        snapshot: &Path,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, String> {
        let inventory = spellwake_content::snapshot::load(metadata, snapshot)
            .map_err(|e| format!("interaction source: {e:?}"))?;
        let registry = Registry::from_interaction_inventory(&inventory, vocabulary)
            .map_err(|e| format!("interaction registry: {e}"))?;
        let inputs = inputs()?;
        super::catalogue::validate_shape(&inputs.definitions, &inputs.decks, &inputs.inventory)?;
        Ok(Self {
            inventory,
            registry,
            inputs,
        })
    }
    /// Build complete physical instances, then apply the explicitly named local setup.
    /// # Errors
    /// Rejects any source, alias, Program, partition, initialization or shuffle mismatch.
    pub fn fixture(
        &self,
        case: InteractionCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Fixture, String> {
        let recipe = self
            .inputs
            .cases
            .iter()
            .find(|c| c.id == case.as_str())
            .ok_or("interaction case missing")?;
        if self.inventory.compressed_sha256 != TEACHING_SOURCE_SHA256
            || self.registry.coverage().registry_version != INTERACTION_REGISTRY_VERSION
            || self.registry.coverage().creature_vocabulary_id != vocabulary.id()
        {
            return Err("interaction source/registry/vocabulary mismatch".into());
        }
        let Materialized {
            aliases,
            cards,
            definitions,
        } = super::catalogue::materialize(
            &self.inventory,
            &self.registry,
            &self.inputs.definitions,
            &self.inputs.decks,
            &self.inputs.inventory,
            ["U", "G"],
        )?;
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context_inputs = ContextInputs {
            reference: format!("interaction-context-v1-{}", case.as_str()),
            release: INTERACTION_PROFILE.into(),
            rules: vocabulary.rules_revision().into(),
            creature_vocabulary: vocabulary.id(),
            name_rules_id: names.id(),
            catalogue: format!("oracle-sha256:{}", self.inventory.compressed_sha256),
            programs: format!("{INTERACTION_PROFILE}:registry{INTERACTION_REGISTRY_VERSION}"),
            profile: format!("modern-bo1-blue-interaction-v1:{}", case.as_str()),
        };
        let context = GameContext::new(
            context_inputs.release.clone(),
            context_inputs.rules.clone(),
            context_inputs.catalogue.clone(),
            context_inputs.programs.clone(),
            context_inputs.profile.clone(),
            vocabulary.id(),
            names.id(),
        )
        .map_err(|e| format!("interaction context: {e:?}"))?;
        let mut fixture = Fixture {
            case: CaseId::Interaction(case),
            initialization: FixtureInitialization::Opening,
            game: GameId(case.game_id()),
            context,
            context_inputs,
            setup: OpeningSetup {
                seed: GameSeed::with_uniform_life(
                    vec![A, B],
                    seat(&recipe.starting_role)?,
                    cards,
                    vec![],
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    vocabulary,
                    names,
                ),
                entropy: EntropyTape::new(vec![]),
            },
            aliases,
            definitions,
            shuffles: vec![],
        };
        if let Some(p) = &recipe.prepared {
            fixture.initialization = FixtureInitialization::Scheduled(TurnStart {
                number: p.individual_turn,
                step: step(&p.step)?,
            });
            if seat(&p.active_role)? != fixture.setup.seed.active {
                return Err("prepared active mismatch".into());
            }
            step(&p.initial_step)?;
            let mut seeds = BTreeMap::new();
            for (zone, roles) in [
                ("library", &p.library_top_first),
                ("hand", &p.hand),
                ("graveyard", &p.graveyard_top_first),
                ("battlefield", &p.battlefield),
            ] {
                if roles.len() != 2 {
                    return Err("prepared role partition mismatch".into());
                }
                for (r, list) in roles {
                    let owner = seat(r)?;
                    // Graveyard insertion prepends; library-bottom insertion appends.
                    let ordered = if zone == "graveyard" {
                        list.iter().rev().collect::<Vec<_>>()
                    } else {
                        list.iter().collect()
                    };
                    for alias in ordered {
                        let id = fixture.card(alias)?;
                        if fixture.alias(id)?.owner != owner
                            || seeds.insert(id, (zone, owner)).is_some()
                        {
                            return Err("prepared duplicate/foreign alias".into());
                        }
                    }
                }
            }
            if seeds.len() != 120 {
                return Err("prepared physical partition incomplete".into());
            }
            let mut assigned = vec![];
            for (zone, roles) in [
                ("library", &p.library_top_first),
                ("hand", &p.hand),
                ("graveyard", &p.graveyard_top_first),
                ("battlefield", &p.battlefield),
            ] {
                for (r, list) in roles {
                    let owner = seat(r)?;
                    let ordered = if zone == "graveyard" {
                        list.iter().rev().collect::<Vec<_>>()
                    } else {
                        list.iter().collect()
                    };
                    for alias in ordered {
                        let id = fixture.card(alias)?;
                        let mut seed = fixture
                            .setup
                            .seed
                            .cards
                            .iter()
                            .find(|s| s.card == id)
                            .ok_or("prepared admitted seed missing")?
                            .clone();
                        seed.destination = match zone {
                            "library" => ObjectDestination::LibraryBottom,
                            "hand" => ObjectDestination::Hand,
                            "graveyard" => ObjectDestination::Graveyard,
                            "battlefield" => ObjectDestination::Battlefield { controller: owner },
                            _ => unreachable!("fixed local zone list"),
                        };
                        assigned.push(seed);
                    }
                }
            }
            fixture.setup.seed.cards = assigned;
            fixture.setup.seed.initial_battlefield_order = p
                .initial_battlefield_order
                .iter()
                .map(|a| fixture.card(a))
                .collect::<Result<Vec<_>, _>>()?;
            let actual = fixture
                .setup
                .seed
                .cards
                .iter()
                .filter(|s| matches!(s.destination, ObjectDestination::Battlefield { .. }))
                .map(|s| s.card)
                .collect::<BTreeSet<_>>();
            if fixture.setup.seed.initial_battlefield_order.len() != actual.len()
                || fixture
                    .setup
                    .seed
                    .initial_battlefield_order
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    != actual
            {
                return Err("prepared exact battlefield order mismatch".into());
            }
        } else {
            for player in [
                seat(&recipe.starting_role)?,
                other(seat(&recipe.starting_role)?)?,
            ] {
                let before = fixture
                    .setup
                    .seed
                    .cards
                    .iter()
                    .filter(|s| s.owner == player)
                    .map(|s| s.card)
                    .collect();
                let target = recipe
                    .shuffle_target_orders_top_first
                    .get(role(player)?)
                    .ok_or("interaction shuffle missing")?;
                fixture.add_shuffle(
                    format!("{}.{}.initial", case.as_str(), role(player)?),
                    player,
                    before,
                    target,
                )?;
            }
            fixture.setup.entropy = EntropyTape::new(
                fixture
                    .shuffles
                    .iter()
                    .flat_map(|s| s.words.iter().copied())
                    .collect(),
            );
        }
        Ok(fixture)
    }
    /// Execute selected inputs through the existing actual recorder and full rerun.
    /// # Errors
    /// Rejects empty selection or invalid source/fixture inputs; failed runs keep evidence.
    pub fn run(
        &self,
        cases: &[InteractionCase],
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<InteractionReport, String> {
        if cases.is_empty() {
            return Err("interaction suite requires cases".into());
        }
        let mut runs = vec![];
        for (index, case) in cases.iter().enumerate() {
            runs.push(record::run_fixture_at(
                self.fixture(*case, vocabulary.clone())?,
                index,
            ));
        }
        Ok(InteractionReport {
            schema_version: 1,
            profile: INTERACTION_PROFILE,
            registry_version: INTERACTION_REGISTRY_VERSION,
            catalogue_sha256: self.inventory.compressed_sha256.clone(),
            runs,
        })
    }
}
/// Privileged finite evidence, not a full-format or durable replay qualification.
#[derive(Debug, Serialize)]
pub struct InteractionReport {
    schema_version: u32,
    profile: &'static str,
    registry_version: u32,
    catalogue_sha256: String,
    pub runs: Vec<RunReport>,
}
impl InteractionReport {
    pub fn completed(&self) -> bool {
        !self.runs.is_empty() && self.runs.iter().all(RunReport::completed)
    }
}

#[derive(Debug, Clone)]
pub(super) enum Intent {
    Action(usize),
    Declaration(usize),
    Attack(usize),
    Block(usize),
    Pass,
    EmptyBlock,
}
#[derive(Debug, Clone)]
struct CastHandle {
    entry: StackEntryId,
    object: ObjectId,
    alias: String,
}
/// One finite policy cursor; rules state stays in GameState, not in this wrapper.
#[derive(Debug, Clone)]
pub(super) struct InteractionPolicy {
    fixture: Fixture,
    recipe: CaseInput,
    consumed: BTreeSet<String>,
    casts: BTreeMap<String, CastHandle>,
    settled: BTreeSet<String>,
    departed: BTreeMap<spellwake_engine::object_identity::CardId, ObjectId>,
    accepted: usize,
}
impl InteractionPolicy {
    pub(super) fn new(fixture: &Fixture) -> Result<Option<Self>, String> {
        let CaseId::Interaction(case) = fixture.case else {
            return Ok(None);
        };
        let recipe = inputs()?
            .cases
            .into_iter()
            .find(|c| c.id == case.as_str())
            .ok_or("interaction recipe missing")?;
        Ok(Some(Self {
            fixture: fixture.clone(),
            recipe,
            consumed: BTreeSet::new(),
            casts: BTreeMap::new(),
            settled: BTreeSet::new(),
            departed: BTreeMap::new(),
            accepted: 0,
        }))
    }
    pub(super) fn entry_id(&self, state: &GameState, intent: &Intent) -> String {
        match intent {
            Intent::Action(i) => self.recipe.actions[*i].id.clone(),
            _ => format!("{}.automatic.{}", self.recipe.id, state.basis().revision),
        }
    }
    pub(super) fn accept(
        &mut self,
        input: &GameInput,
        candidate: &NativeCandidate,
        intent: Intent,
    ) -> Result<(), String> {
        if let Intent::Action(i) = &intent {
            let a = &self.recipe.actions[*i];
            if matches!(a.kind, ActionKind::Cast) {
                let alias = a.card.as_ref().ok_or("cast alias missing")?;
                let cast = super::policy::accepted_cast(&self.fixture, input, candidate, alias)?;
                if self
                    .casts
                    .insert(
                        a.id.clone(),
                        CastHandle {
                            entry: cast.entry,
                            object: cast.object,
                            alias: alias.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err("cast action reused".into());
                }
            }
        }
        // Exiting physical spells publish the same real zone owner for every body kind.
        for record in &candidate.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::ZoneChange(change) = &occurrence.value
                        && change.before.object.zone() == ObjectZone::Battlefield
                        && let Some(card) = change.before.object.card_id()
                    {
                        self.departed.insert(card, change.before.object.id());
                    }
                    if let RulesOccurrence::ZoneChange(change) = &occurrence.value
                        && change.before.object.zone() == ObjectZone::Stack
                    {
                        for (id, cast) in &self.casts {
                            if cast.object == change.before.object.id() {
                                self.settled.insert(id.clone());
                            }
                        }
                    }
                }
            }
        }
        for declaration in candidate.combat_declarations() {
            if let spellwake_engine::combat::CombatDeclarationRecord::Attackers {
                player,
                origin: spellwake_engine::combat::DeclarationOrigin::ForcedEmpty,
                attackers,
                ..
            } = declaration
            {
                let own_turn = candidate
                    .state()
                    .turn()
                    .ok_or("forced attackers lack turn")?
                    .position()
                    .number
                    .div_ceil(2);
                if let Some((i, c)) = self.recipe.combat.iter().enumerate().find(|(_, c)| {
                    seat(&c.active_role).ok() == Some(*player) && c.own_turn == own_turn
                }) {
                    if !c.expected_eligible_attackers.is_empty()
                        || !c.attackers.is_empty()
                        || !attackers.is_empty()
                    {
                        return Err(
                            "forced empty attackers differ from authored eligibility".into()
                        );
                    }
                    if !self.consumed.insert(format!("attack.{i}")) {
                        return Err("forced attack boundary consumed twice".into());
                    }
                }
            }
        }
        let key = match intent {
            Intent::Action(i) => Some(format!("action.{i}")),
            Intent::Declaration(i) => Some(format!("declaration.{i}")),
            Intent::Attack(i) => Some(format!("attack.{i}")),
            Intent::Block(i) => Some(format!("block.{i}")),
            Intent::Pass | Intent::EmptyBlock => None,
        };
        if let Some(key) = key
            && !self.consumed.insert(key)
        {
            return Err("interaction intent consumed twice".into());
        }
        self.accepted += 1;
        Ok(())
    }
    fn actions_complete(&self) -> bool {
        (0..self.recipe.actions.len()).all(|i| self.consumed.contains(&format!("action.{i}")))
    }
    pub(super) fn completed_boundary(&self, state: &GameState) -> Result<Option<StopKind>, String> {
        let Stop::ReadyPriority {
            individual_turn,
            step: expected_step,
            active_role,
            priority_holder,
            stack,
            all_actions_consumed,
            required_settlements,
            boundary_ref,
        } = &self.recipe.stop
        else {
            return Ok(None);
        };
        if state.outcome().is_some() {
            return Err("bounded interaction case ended naturally before its stop".into());
        }
        if !stack.is_empty() || !all_actions_consumed {
            return Err("invalid bounded interaction stop contract".into());
        }
        if !self.actions_complete() {
            return Ok(None);
        }
        if required_settlements
            .iter()
            .any(|id| !self.settled.contains(id))
        {
            return Ok(None);
        }
        let turn = state.turn().ok_or("bounded case has no turn")?.position();
        if turn.number != *individual_turn || turn.step != step(expected_step)? {
            return Ok(None);
        }
        let w = state.execution().window();
        if !w.entries().is_empty()
            || state.pending_decision().is_some()
            || state.pending_turn_work().is_some()
            || state
                .turn()
                .is_none_or(|t| t.stage() != spellwake_engine::turn::TurnStage::Priority)
            || state.execution().resolution_state().is_some()
        {
            return Ok(None);
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = w.phase() else {
            return Ok(None);
        };
        let active = w.turn_owner();
        if active != seat(active_role)? || holder != seat(priority_holder)? {
            return Ok(None);
        }
        let CaseId::Interaction(case) = self.fixture.case else {
            return Err("interaction case tag changed".into());
        };
        if boundary_ref != case.boundary_ref() {
            return Err("bounded stop ref differs from case".into());
        }
        Ok(Some(StopKind::PreparedReadyPriority {
            boundary_ref: case.boundary_ref(),
            position: turn,
            active,
            holder,
        }))
    }
    pub(super) fn finish(&self, state: &GameState) -> Result<(), String> {
        if !self.actions_complete() {
            return Err("unfinished interaction actions/issued input".into());
        }
        for i in 0..self.recipe.opening_declarations.len() {
            if !self.consumed.contains(&format!("declaration.{i}")) {
                return Err("unconsumed interaction declaration".into());
            }
        }
        for (i, c) in self.recipe.combat.iter().enumerate() {
            if !self.consumed.contains(&format!("attack.{i}"))
                || (!c.blockers.is_empty() && !self.consumed.contains(&format!("block.{i}")))
            {
                return Err(format!("unconsumed interaction combat {i}"));
            }
        }
        if self.casts.keys().any(|id| !self.settled.contains(id)) {
            return Err("interaction cast lacks actual settlement".into());
        }
        let expected_turn = self
            .recipe
            .prepared
            .as_ref()
            .map_or(16, |p| p.individual_turn);
        if state
            .turn()
            .is_none_or(|t| t.position().number != expected_turn)
        {
            return Err("interaction completion turn mismatch".into());
        }
        match &self.recipe.stop {
            Stop::NaturalOutcome => {
                if state.outcome().is_none() {
                    return Err("interaction natural outcome missing".into());
                }
            }
            Stop::ReadyPriority { .. } => {
                if self.completed_boundary(state)?.is_none() {
                    return Err("interaction bounded stop missing".into());
                }
            }
        }
        match self.fixture.initialization {
            FixtureInitialization::Opening => {
                if state.entropy().is_none_or(|t| t.remaining() != 0) {
                    return Err("interaction opening entropy not exhausted".into());
                }
            }
            FixtureInitialization::Scheduled(_) => {
                if state.entropy().is_some() {
                    return Err("prepared interaction unexpectedly has entropy".into());
                }
            }
        }
        Ok(())
    }
    fn objects(&self, state: &GameState, aliases: &[String]) -> Result<Vec<ObjectId>, String> {
        aliases
            .iter()
            .map(|a| self.fixture.current_object(state, a))
            .collect()
    }
    fn domain(
        &self,
        state: &GameState,
        actual: &[ObjectId],
        aliases: &[String],
    ) -> Result<(), String> {
        let expected = self
            .objects(state, aliases)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        if actual.len() != expected.len()
            || actual.iter().copied().collect::<BTreeSet<_>>() != expected
        {
            return Err(format!(
                "interaction offered domain mismatch: expected {aliases:?}, actual {actual:?}"
            ));
        }
        Ok(())
    }
    fn creature_domain(
        &self,
        state: &GameState,
        owner: SeatId,
        attack: bool,
    ) -> Result<Vec<String>, String> {
        let mut aliases = vec![];
        for alias in self.fixture.aliases.values().filter(|a| a.owner == owner) {
            let id = self.fixture.current_object(state, &alias.alias)?;
            let card = state
                .execution()
                .window()
                .cards()
                .get(id)
                .ok_or("domain object missing")?;
            let program = state
                .execution()
                .programs()
                .find(|(id, _)| *id == alias.card)
                .map(|(_, p)| p)
                .ok_or("domain Program missing")?;
            if program.has_type(spellwake_engine::program::CardType::Creature)
                && card.zone() == ObjectZone::Battlefield
                && card.controller() == Some(owner)
                && card.tapped() == Some(false)
                && (!attack || card.has_continuous_control_since_turn_start() == Some(true))
            {
                aliases.push(alias.alias.clone());
            }
        }
        Ok(aliases)
    }
    fn stack_matches(&self, state: &GameState, a: &Action) -> Result<bool, String> {
        let entries = state.execution().window().entries();
        if entries.len() != a.expected_stack_bottom_first.len() {
            return Ok(false);
        }
        if entries.len() != a.expected_stack_cast_actions_bottom_first.len() {
            return Err("stack physical/named-cast preconditions differ in length".into());
        }
        for ((entry, alias), cast_id) in entries
            .iter()
            .zip(&a.expected_stack_bottom_first)
            .zip(&a.expected_stack_cast_actions_bottom_first)
        {
            let Some(cast) = self.casts.get(cast_id) else {
                return Ok(false);
            };
            if cast.alias != *alias
                || cast.entry != entry.id()
                || !matches!(entry.kind(),StackKind::CardSpell(id) if *id==cast.object)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn action_command(&self, state: &GameState, a: &Action) -> Result<GameCommand, String> {
        let object = || -> Result<ObjectId, String> {
            self.fixture
                .current_object(state, a.card.as_deref().ok_or("action card missing")?)
        };
        match a.kind {
            ActionKind::PassPriority => Ok(GameCommand::Pass),
            ActionKind::PlayLand => Ok(GameCommand::PlayLand { land: object()? }),
            ActionKind::ActivateIntrinsicMana => Ok(GameCommand::ActivateMana {
                source: object()?,
                ability: ManaAbilityRef::BasicLand(if seat(&a.actor_role)? == A {
                    BasicLandType::Island
                } else {
                    BasicLandType::Forest
                }),
            }),
            ActionKind::Cast => {
                if a.payment.is_empty() {
                    return Err("interaction cast payment missing".into());
                }
                let payment = ManaPayment {
                    spend: a
                        .payment
                        .iter()
                        .map(|(color, amount)| {
                            let mana_type = match color.as_str() {
                                "blue" => ManaType::Blue,
                                "green" => ManaType::Green,
                                _ => return Err("unknown interaction mana color".into()),
                            };
                            let amount = RulesInteger::try_from(amount.clone())
                                .map_err(|_| "invalid interaction decimal payment")?;
                            if !amount.is_positive() {
                                return Err("noncanonical/nonpositive interaction payment".into());
                            }
                            Ok(ManaSpend { mana_type, amount })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                };
                let spell = object()?;
                let Some(target) = &a.target else {
                    return Ok(GameCommand::CastUntargeted {
                        spell,
                        payment: Some(payment),
                    });
                };
                let target = match target {
                    Target::Object { card } => {
                        TargetRef::Object(self.fixture.current_object(state, card)?)
                    }
                    Target::Stack { spell } => {
                        let id = a
                            .target_cast_action
                            .as_ref()
                            .ok_or("stack target needs named cast action")?;
                        let cast = self
                            .casts
                            .get(id)
                            .ok_or("stack target named cast not observed")?;
                        if cast.alias!=*spell || !state.execution().window().entries().iter().any(|entry|entry.id()==cast.entry&&matches!(entry.kind(),StackKind::CardSpell(object)if *object==cast.object)){return Err("named stack target is not live".into());}
                        TargetRef::Stack(cast.entry)
                    }
                };
                Ok(GameCommand::CastPaid {
                    spell,
                    target,
                    payment,
                })
            }
            ActionKind::DeclareAttackers | ActionKind::DeclareBlockers => {
                Err("combat action requires actual bound decision".into())
            }
        }
    }
    pub(super) fn prepare(&self, state: &GameState) -> Result<(GameInput, Intent), String> {
        if state.basis().game != self.fixture.game
            || state.basis().context != self.fixture.context
            || state.outcome().is_some()
        {
            return Err("interaction game/context/outcome mismatch".into());
        }
        if state.turn().is_some_and(|t| t.position().number > 20) {
            return Err("interaction turn watchdog exhausted".into());
        }
        if self.accepted == 0
            && let Some(p) = &self.recipe.prepared
        {
            let pos = state
                .turn()
                .ok_or("prepared initial candidate has no turn")?
                .position();
            if pos.number != p.individual_turn
                || pos.step != step(&p.initial_step)?
                || state.pending_decision().is_some()
            {
                return Err("prepared actual initialization boundary mismatch".into());
            }
        }
        let next = self
            .recipe
            .actions
            .iter()
            .enumerate()
            .find(|(i, _)| !self.consumed.contains(&format!("action.{i}")));
        let (actor, command, intent) = if let Some(pending) = state.pending_decision() {
            let actor = pending.chooser();
            let (choice, intent) = match pending.frame() {
                RuleFrame::Opening(OpeningDecision::Declare { player, taken, .. }) => {
                    let (i, d) = self
                        .recipe
                        .opening_declarations
                        .iter()
                        .enumerate()
                        .find(|(i, d)| {
                            seat(&d.role).ok() == Some(*player)
                                && d.taken == *taken
                                && !self.consumed.contains(&format!("declaration.{i}"))
                        })
                        .ok_or("unexpected interaction opening declaration")?;
                    if d.choice != "keep" {
                        return Err("interaction opening recipe only keeps".into());
                    }
                    (
                        DecisionReply::Mulligan(MulliganChoice::Keep),
                        Intent::Declaration(i),
                    )
                }
                RuleFrame::Combat(CombatDecision::Attackers(frame)) => {
                    let turn = state
                        .turn()
                        .ok_or("attack decision missing turn")?
                        .position();
                    let authored = self.recipe.combat.iter().enumerate().find(|(_, c)| {
                        seat(&c.active_role).ok() == Some(actor)
                            && c.own_turn == turn.number.div_ceil(2)
                    });
                    let (aliases, eligible, intent) = if let Some((i, c)) = authored {
                        (
                            c.attackers.clone(),
                            c.expected_eligible_attackers.clone(),
                            Intent::Attack(i),
                        )
                    } else if let Some((i, a)) = next
                        && matches!(a.kind, ActionKind::DeclareAttackers)
                        && a.individual_turn == turn.number
                        && seat(&a.actor_role)? == actor
                    {
                        (
                            a.attackers.clone(),
                            self.creature_domain(state, actor, true)?,
                            Intent::Action(i),
                        )
                    } else {
                        return Err("unexpected interaction attacker frame".into());
                    };
                    self.domain(state, frame.eligible_attackers(), &eligible)?;
                    if frame.defenders() != [other(actor)?] {
                        return Err("interaction defender domain mismatch".into());
                    }
                    let defender = other(actor)?;
                    (
                        DecisionReply::DeclareAttackers(
                            self.objects(state, &aliases)?
                                .into_iter()
                                .map(|attacker| AttackDeclaration { attacker, defender })
                                .collect(),
                        ),
                        intent,
                    )
                }
                RuleFrame::Combat(CombatDecision::Blockers(frame)) => {
                    let turn = state
                        .turn()
                        .ok_or("block decision missing turn")?
                        .position();
                    let active = state.execution().window().turn_owner();
                    let authored = self.recipe.combat.iter().enumerate().find(|(_, c)| {
                        seat(&c.active_role).ok() == Some(active)
                            && c.own_turn == turn.number.div_ceil(2)
                    });
                    let (blocks, attackers, intent) = if let Some((i, c)) = authored {
                        (c.blockers.clone(), c.attackers.clone(), Intent::Block(i))
                    } else if let Some((i, a)) = next
                        && matches!(a.kind, ActionKind::DeclareBlockers)
                        && a.individual_turn == turn.number
                        && seat(&a.actor_role)? == actor
                    {
                        (
                            a.blockers.clone(),
                            a.blockers.iter().map(|b| b.attacker.clone()).collect(),
                            Intent::Action(i),
                        )
                    } else if self.recipe.prepared.is_some() && frame.attackers().is_empty() {
                        (vec![], vec![], Intent::EmptyBlock)
                    } else {
                        return Err("unexpected interaction blocker frame".into());
                    };
                    self.domain(state, frame.attackers(), &attackers)?;
                    self.domain(
                        state,
                        frame.eligible_blockers(),
                        &self.creature_domain(state, actor, false)?,
                    )?;
                    let blocks = blocks
                        .iter()
                        .map(|b| {
                            Ok(BlockDeclaration {
                                blocker: self.fixture.current_object(state, &b.blocker)?,
                                attacker: self.fixture.current_object(state, &b.attacker)?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    (DecisionReply::DeclareBlockers(blocks), intent)
                }
                _ => return Err("unexpected interaction decision family".into()),
            };
            (
                actor,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice,
                },
                intent,
            )
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                return Err("interaction requires actual Ready priority or bound decision".into());
            };
            let turn = state.turn().ok_or("priority without turn")?.position();
            let mut selected = None;
            if let Some((i, a)) = next {
                if turn.number > a.individual_turn {
                    return Err(format!("expired interaction action {}", a.id));
                }
                if turn.number == a.individual_turn
                    && turn.step == step(&a.step)?
                    && state.execution().window().turn_owner() == seat(&a.active_role)?
                    && holder == seat(&a.actor_role)?
                    && self.stack_matches(state, a)?
                    && a.after_settled_action
                        .as_ref()
                        .is_none_or(|id| self.settled.contains(id))
                {
                    selected = Some((self.action_command(state, a)?, Intent::Action(i)));
                }
            }
            let (command, intent) = selected.unwrap_or((GameCommand::Pass, Intent::Pass));
            (holder, command, intent)
        };
        Ok((
            GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            },
            intent,
        ))
    }
}
/// Alter only actual reached inputs. The shared recorder executes and reruns probes.
pub(super) fn reached_probes(
    fixture: &Fixture,
    policy: &Policy,
    state: &GameState,
    prepared: Option<&super::policy::PreparedInput>,
) -> Result<Vec<RejectionProbe>, String> {
    use spellwake_engine::{
        actions::TimingError, combat::CombatError, effects::EffectError, game::GameError,
        mana::ManaPaymentError, turn::TurnError,
    };
    if state.outcome().is_some() || policy.completed_boundary(state)?.is_some() {
        return Ok(vec![]);
    }
    let valid = &prepared
        .ok_or("nonterminal reached probes require the one prepared input")?
        .input;
    let interaction = policy
        .interaction()
        .ok_or("interaction probe policy missing")?;
    let mut probes = vec![];
    let mut add = |family: &str, command: GameCommand, error: GameError| {
        probes.push(RejectionProbe {
            id: format!(
                "{}.{}.{}",
                fixture.case.as_str(),
                family,
                state.basis().revision
            ),
            expectation_ref: format!("IF13.{family}"),
            attachment: "actual-interaction-reached-state".into(),
            input: GameInput {
                basis: valid.basis.clone(),
                actor: valid.actor,
                command,
            },
            expected: error,
        });
    };
    if let GameCommand::CastPaid {
        spell,
        target,
        payment,
    } = &valid.command
    {
        let alias = state
            .execution()
            .window()
            .cards()
            .get(*spell)
            .and_then(|o| o.card_id())
            .map(|id| fixture.alias(id))
            .transpose()?
            .ok_or("probe spell physical alias missing")?;
        let name = fixture
            .definitions
            .iter()
            .find(|d| d.reference == alias.definition_ref)
            .ok_or("probe definition absent")?
            .name
            .as_str();
        let targeted = |target| GameCommand::CastPaid {
            spell: *spell,
            target,
            payment: payment.clone(),
        };
        let illegal = GameError::Effect(EffectError::IllegalTarget);
        add(
            &format!("wrong-target-count.{}.missing", name.to_ascii_lowercase()),
            GameCommand::CastUntargeted {
                spell: *spell,
                payment: Some(payment.clone()),
            },
            GameError::Effect(EffectError::TargetRequired),
        );
        if matches!(name, "Counterspell" | "Cancel") {
            // Distinguish a short selected spend from an underfunded pool.
            // These are the exact frozen IF13 attempts, after all real taps.
            let blue_only = ManaType::ALL.into_iter().all(|color| {
                color == ManaType::Blue || state.execution().mana(valid.actor, color) == 0.into()
            });
            if blue_only
                && matches!(target, TargetRef::Stack(entry) if state.execution().window().entries().iter().any(|candidate| candidate.id() == *entry && matches!(candidate.kind(), StackKind::CardSpell(_))))
            {
                let attempted = match name {
                    "Counterspell"
                        if state.execution().mana(valid.actor, ManaType::Blue) == 2.into() =>
                    {
                        Some((
                            "counter-insufficient-payment",
                            1,
                            ManaPaymentError::MissingRequired,
                        ))
                    }
                    "Cancel" if state.execution().mana(valid.actor, ManaType::Blue) == 3.into() => {
                        Some((
                            "cancel-insufficient-payment",
                            2,
                            ManaPaymentError::WrongTotal,
                        ))
                    }
                    _ => None,
                };
                if let Some((family, units, error)) = attempted {
                    add(
                        family,
                        GameCommand::CastPaid {
                            spell: *spell,
                            target: *target,
                            payment: ManaPayment {
                                spend: vec![ManaSpend {
                                    mana_type: ManaType::Blue,
                                    amount: units.into(),
                                }],
                            },
                        },
                        GameError::Effect(EffectError::InvalidManaPayment(error)),
                    );
                }
            }
            for (family, object) in [
                (
                    "counter-creature-permanent",
                    fixture.current_object(state, "G.grizzly-bears.1")?,
                ),
                (
                    "counter-land-permanent",
                    fixture.current_object(state, "U.island.1")?,
                ),
            ] {
                if state
                    .execution()
                    .window()
                    .cards()
                    .get(object)
                    .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
                {
                    add(family, targeted(TargetRef::Object(object)), illegal.clone());
                }
            }
            add(
                "counter-card-in-hand",
                targeted(TargetRef::Object(*spell)),
                illegal.clone(),
            );
            if let Some(cast) = interaction.casts.values().find(|cast| {
                !state
                    .execution()
                    .window()
                    .entries()
                    .iter()
                    .any(|e| e.id() == cast.entry)
            }) {
                add(
                    "counter-completed-entry",
                    targeted(TargetRef::Stack(cast.entry)),
                    illegal.clone(),
                );
            }
            if name == "Cancel" {
                add(
                    "unavailable-mana-color",
                    GameCommand::CastPaid {
                        spell: *spell,
                        target: match &valid.command {
                            GameCommand::CastPaid { target, .. } => *target,
                            _ => unreachable!("outer paid cast"),
                        },
                        payment: ManaPayment {
                            spend: vec![
                                ManaSpend {
                                    mana_type: ManaType::Blue,
                                    amount: 2.into(),
                                },
                                ManaSpend {
                                    mana_type: ManaType::Green,
                                    amount: 1.into(),
                                },
                            ],
                        },
                    },
                    GameError::Effect(EffectError::InsufficientMana),
                );
            }
        } else if name == "Unsummon" {
            let land = fixture.current_object(state, "U.island.1")?;
            if state
                .execution()
                .window()
                .cards()
                .get(land)
                .is_some_and(|o| o.zone() == ObjectZone::Battlefield)
            {
                add(
                    "unsummon-land",
                    targeted(TargetRef::Object(land)),
                    illegal.clone(),
                );
            }
            for (family, zone) in [
                ("unsummon-creature-spell", ObjectZone::Stack),
                ("unsummon-card-in-hand", ObjectZone::Hand(valid.actor)),
            ] {
                if let Some(seed) = fixture.setup.seed.cards.iter().find(|s| {
                    s.program
                        .has_type(spellwake_engine::program::CardType::Creature)
                        && state
                            .execution()
                            .window()
                            .cards()
                            .current_identity(s.card)
                            .and_then(|id| state.execution().window().cards().get(id))
                            .is_some_and(|o| o.zone() == zone)
                }) {
                    add(
                        family,
                        targeted(TargetRef::Object(
                            state
                                .execution()
                                .window()
                                .cards()
                                .current_identity(seed.card)
                                .ok_or("probe creature missing")?,
                        )),
                        illegal.clone(),
                    );
                }
            }
            if let Some(old) = interaction.departed.values().next() {
                add(
                    "unsummon-stale-incarnation",
                    targeted(TargetRef::Object(*old)),
                    illegal,
                );
            }
        }
        let mut wrong = payment.clone();
        if let Some(first) = wrong.spend.first_mut() {
            first.amount = 0.into();
        }
        add(
            "counter-invalid-payment",
            GameCommand::CastPaid {
                spell: *spell,
                target: match &valid.command {
                    GameCommand::CastPaid { target, .. } => *target,
                    _ => unreachable!("outer paid cast"),
                },
                payment: wrong,
            },
            GameError::Effect(EffectError::InvalidManaPayment(
                ManaPaymentError::NonPositiveSpend,
            )),
        );
    }
    if let GameCommand::CastUntargeted {
        spell,
        payment: Some(payment),
    } = &valid.command
        && state
            .execution()
            .window()
            .cards()
            .get(*spell)
            .and_then(|o| o.card_id())
            .and_then(|id| fixture.aliases.get(&id))
            .is_some_and(|a| a.alias.contains("divination"))
    {
        add(
            "divination-extra-target",
            GameCommand::CastPaid {
                spell: *spell,
                target: TargetRef::Player(valid.actor),
                payment: payment.clone(),
            },
            GameError::Effect(EffectError::UnexpectedTarget),
        );
    }
    if matches!(valid.command, GameCommand::ActivateMana { .. }) {
        let position = state.turn().ok_or("mana probe lacks turn")?.position();
        if let Some(a) = interaction
            .recipe
            .actions
            .iter()
            .enumerate()
            .filter(|(i, _)| !interaction.consumed.contains(&format!("action.{i}")))
            .map(|(_, a)| a)
            .find(|a| {
                matches!(a.kind, ActionKind::Cast)
                    && seat(&a.actor_role).ok() == Some(valid.actor)
                    && a.individual_turn == position.number
            })
            && let Ok(command) = interaction.action_command(state, a)
        {
            let spend = match &command {
                GameCommand::CastPaid { payment, .. }
                | GameCommand::CastUntargeted {
                    payment: Some(payment),
                    ..
                } => &payment.spend,
                _ => return Err("probe cast has no payment".into()),
            };
            if spend
                .iter()
                .any(|s| state.execution().mana(valid.actor, s.mana_type) < s.amount)
            {
                let family = if a.card.as_ref().is_some_and(|a| a.contains("counterspell")) {
                    "counter-insufficient-pool"
                } else if a.card.as_ref().is_some_and(|a| a.contains("cancel")) {
                    "cancel-insufficient-pool"
                } else {
                    "cast-insufficient-pool"
                };
                add(
                    family,
                    command,
                    GameError::Effect(EffectError::InsufficientMana),
                );
            }
        }
    }
    if let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
        state.execution().window().phase()
    {
        let turn = state.turn().ok_or("probe priority lacks turn")?;
        let active = state.execution().window().turn_owner();
        if holder == active
            && matches!(
                turn.position().step,
                Step::PrecombatMain | Step::PostcombatMain
            )
        {
            let hand_creature = fixture.setup.seed.cards.iter().find(|s| {
                interaction.departed.contains_key(&s.card)
                    && s.owner == holder
                    && s.program
                        .has_type(spellwake_engine::program::CardType::Creature)
                    && state
                        .execution()
                        .window()
                        .cards()
                        .current_identity(s.card)
                        .and_then(|id| state.execution().window().cards().get(id))
                        .is_some_and(|o| o.zone() == ObjectZone::Hand(holder))
            });
            if !state.execution().window().entries().is_empty()
                && let Some(seed) = hand_creature
                && interaction.departed.contains_key(&seed.card)
            {
                add(
                    "creature-recast-before-lower-bounce-completes",
                    GameCommand::CastUntargeted {
                        spell: state
                            .execution()
                            .window()
                            .cards()
                            .current_identity(seed.card)
                            .ok_or("recast probe missing")?,
                        payment: None,
                    },
                    GameError::Effect(EffectError::Timing(TimingError::StackNotEmpty)),
                );
            }
            if turn.lands_played().is_positive()
                && let Some(seed) = fixture.setup.seed.cards.iter().find(|s| {
                    s.owner == holder
                        && s.program
                            .has_type(spellwake_engine::program::CardType::Land)
                        && state
                            .execution()
                            .window()
                            .cards()
                            .current_identity(s.card)
                            .and_then(|id| state.execution().window().cards().get(id))
                            .is_some_and(|o| o.zone() == ObjectZone::Hand(holder))
                })
            {
                add(
                    "second-land",
                    GameCommand::PlayLand {
                        land: state
                            .execution()
                            .window()
                            .cards()
                            .current_identity(seed.card)
                            .ok_or("land probe missing")?,
                    },
                    GameError::Turn(TurnError::LandLimitReached),
                );
            }
        }
    }
    if let Some(pending) = state.pending_decision()
        && matches!(
            pending.frame(),
            RuleFrame::Combat(CombatDecision::Attackers(_))
        )
    {
        for alias in fixture.aliases.values().filter(|a| a.owner == valid.actor) {
            let current = fixture.current_object(state, &alias.alias)?;
            if state
                .execution()
                .window()
                .cards()
                .get(current)
                .is_some_and(|o| {
                    o.zone() == ObjectZone::Battlefield
                        && o.has_continuous_control_since_turn_start() == Some(false)
                })
                && fixture.setup.seed.cards.iter().any(|s| {
                    s.card == alias.card
                        && s.program
                            .has_type(spellwake_engine::program::CardType::Creature)
                })
            {
                add(
                    "recast-young-attacker.fresh-object",
                    GameCommand::Reply {
                        decision: pending.id(),
                        choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                            attacker: current,
                            defender: other(valid.actor)?,
                        }]),
                    },
                    GameError::Effect(EffectError::Combat(CombatError::IllegalAttacker)),
                );
                if let Some(old) = interaction.departed.get(&alias.card) {
                    add(
                        "recast-young-attacker.stale-object",
                        GameCommand::Reply {
                            decision: pending.id(),
                            choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                                attacker: *old,
                                defender: other(valid.actor)?,
                            }]),
                        },
                        GameError::Effect(EffectError::Combat(CombatError::IllegalAttacker)),
                    );
                }
                break;
            }
        }
    }
    if state.basis().revision > 0 {
        let mut input = valid.clone();
        input.basis.revision -= 1;
        probes.push(RejectionProbe {
            id: format!("{}.stale.{}", fixture.case.as_str(), state.basis().revision),
            expectation_ref: "IF13.stale-seat-revision".into(),
            attachment: "actual-interaction-reached-state".into(),
            input,
            expected: spellwake_engine::game::GameError::StaleRevision,
        });
    }
    if matches!(valid.command, GameCommand::Pass)
        && let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
    {
        if holder != valid.actor {
            return Err("offered pass actor is not actual priority holder".into());
        }
        let mut input = valid.clone();
        input.actor = other(holder)?;
        probes.push(RejectionProbe {
            id: format!(
                "{}.foreign-priority.{}",
                fixture.case.as_str(),
                state.basis().revision
            ),
            expectation_ref: "IF13.foreign-actor".into(),
            attachment: "actual-interaction-reached-state".into(),
            input,
            expected: GameError::Effect(EffectError::Window(
                spellwake_engine::stack_window::StackWindowError::Priority(
                    spellwake_engine::priority::PriorityError::WrongPlayer,
                ),
            )),
        });
    }
    let mut input = valid.clone();
    input.actor = SeatId(999);
    probes.push(RejectionProbe {
        id: format!(
            "{}.unknown-actor.{}",
            fixture.case.as_str(),
            state.basis().revision
        ),
        expectation_ref: "IF13.unknown-actor".into(),
        attachment: "actual-interaction-reached-state".into(),
        input,
        expected: spellwake_engine::game::GameError::UnknownActor,
    });
    Ok(probes)
}
