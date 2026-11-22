//! Catalogue-backed vanilla teaching inputs over the ordinary execution owner.
//! This module supplies commands and records observations; the native engine alone
//! decides legality, state checks and outcomes. Embedded recipes contain no results.
use super::catalogue::{Deck, DefinitionInput, InventoryEntry, slug};
use super::{
    fixture::{A, B, CaseId, ContextInputs, Fixture, FixtureInitialization, other},
    probes::RejectionProbe,
    record::{self, RunReport},
};
use serde::{Deserialize, Serialize};
use spellwake_card_programs::{
    Registry, TEACHING_PROFILE, TEACHING_REGISTRY_VERSION, TEACHING_SOURCE_SHA256,
};
use spellwake_content::Inventory;
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_engine::{
    checks::CheckDecision,
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision, CombatError, CombatRules},
    effects::EffectError,
    game::{
        DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, OpeningSetup,
        RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectId, ObjectZone},
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    turn::{Step, TurnError},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

/// Exact ordinary trajectories frozen by the independent teaching-case recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeachingCase {
    Rd01,
    Rd02,
}
impl TeachingCase {
    pub const ALL: [Self; 2] = [Self::Rd01, Self::Rd02];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rd01 => "RD01",
            Self::Rd02 => "RD02",
        }
    }
    pub(super) const fn game_id(self) -> u128 {
        match self {
            Self::Rd01 => 6001,
            Self::Rd02 => 6002,
        }
    }
    /// Parse the closed case vocabulary. Unknown inputs are rejected.
    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|v| v.as_str() == value)
            .ok_or_else(|| format!("unknown teaching case {value}"))
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
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
    shuffle_target_orders_top_first: BTreeMap<String, Vec<String>>,
    opening_declarations: Vec<Declaration>,
    #[serde(default)]
    mulligan_shuffles: Vec<MulliganShuffle>,
    #[serde(default)]
    london_bottom: Vec<Bottom>,
    actions: Vec<Action>,
    combat: Vec<Combat>,
    #[serde(default)]
    state_check_order_choices: Vec<Order>,
    cleanup_discards: Vec<Discard>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    role: String,
    taken: u8,
    choice: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MulliganShuffle {
    role: String,
    taken_after_shuffle: u8,
    target_order_top_first: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bottom {
    role: String,
    count: usize,
    selected_top_first: Vec<String>,
    hand_domain: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    role: String,
    own_turn: u64,
    step: String,
    kind: String,
    card: String,
    #[serde(default)]
    mana_units_paid: Option<String>,
    #[serde(default)]
    required_cost: Option<String>,
    #[serde(default)]
    targets: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Combat {
    active_role: String,
    own_turn: u64,
    attackers: Vec<String>,
    blockers: Vec<Block>,
    defender_role: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    blocker: String,
    attacker: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Order {
    at: String,
    owner_role: String,
    domain: Vec<String>,
    selected_graveyard_top_first: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Discard {
    role: String,
    own_turn: u64,
    cards: Vec<String>,
    hand_count_before: usize,
    hand_count_after: usize,
}
fn inputs() -> Result<Inputs, String> {
    serde_json::from_str(include_str!("teaching-inputs-v1.json"))
        .map_err(|e| format!("invalid compiled teaching input: {e}"))
}
fn seat(role: &str) -> Result<SeatId, String> {
    match role {
        "W" => Ok(A),
        "G" => Ok(B),
        _ => Err(format!("unknown teaching role {role}")),
    }
}
fn role(player: SeatId) -> Result<&'static str, String> {
    match player {
        A => Ok("W"),
        B => Ok("G"),
        _ => Err("unknown teaching seat".into()),
    }
}

/// Actual hash-checked saved inventory and its guarded teaching registry.
/// The source loader hashes the same bytes it decodes; no fixture input can
/// override the required catalogue/vocabulary identities at this boundary.
pub struct TeachingSource {
    inventory: Inventory,
    registry: Registry,
    inputs: Inputs,
}
impl TeachingSource {
    /// Immutable actual saved inventory admitted for this source.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    /// Immutable source-qualified teaching registry and its coverage witness.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Load the actual saved snapshot and construct all twenty guarded bindings.
    ///
    /// # Errors
    /// Rejects unreadable/invalid bytes and any catalogue, vocabulary or selected
    /// definition mismatch reported by the source/registry boundaries.
    pub fn load(
        metadata: &Path,
        snapshot: &Path,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, String> {
        let inventory = spellwake_content::snapshot::load(metadata, snapshot)
            .map_err(|e| format!("teaching source: {e:?}"))?;
        let registry = Registry::from_teaching_inventory(&inventory, vocabulary)
            .map_err(|e| format!("teaching registry: {e}"))?;
        let inputs = inputs()?;
        super::catalogue::validate_shape(&inputs.definitions, &inputs.decks, &inputs.inventory)?;
        Ok(Self {
            inventory,
            registry,
            inputs,
        })
    }
    /// Materialize library-only physical instances using the guarded registry.
    ///
    /// # Errors
    /// Rejects an unknown case, missing identities/aliases, any unsupported seed,
    /// malformed permutation, or native context construction failure.
    pub fn fixture(
        &self,
        case: TeachingCase,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Fixture, String> {
        let recipe = self
            .inputs
            .cases
            .iter()
            .find(|v| v.id == case.as_str())
            .ok_or("teaching case input missing")?;
        let catalogue = format!("oracle-sha256:{}", self.inventory.compressed_sha256);
        if self.inventory.compressed_sha256 != TEACHING_SOURCE_SHA256
            || self.registry.coverage().registry_version != TEACHING_REGISTRY_VERSION
            || self.registry.coverage().creature_vocabulary_id != vocabulary.id()
        {
            return Err("teaching context differs from actual source/registry/vocabulary".into());
        }
        let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let context_inputs = ContextInputs {
            reference: format!("teaching-context-v1-{}", case.as_str()),
            release: "catalogue-teaching-pair-v1".into(),
            rules: vocabulary.rules_revision().into(),
            creature_vocabulary: vocabulary.id(),
            name_rules_id: names.id(),
            catalogue: catalogue.clone(),
            programs: format!("{TEACHING_PROFILE}:registry{TEACHING_REGISTRY_VERSION}"),
            profile: format!("modern-bo1-teaching-pair-v1:{}", case.as_str()),
        };
        let context = GameContext::new(
            context_inputs.release.clone(),
            context_inputs.rules.clone(),
            catalogue,
            context_inputs.programs.clone(),
            context_inputs.profile.clone(),
            vocabulary.id(),
            names.id(),
        )
        .map_err(|e| format!("teaching context: {e:?}"))?;
        let super::catalogue::Materialized {
            aliases,
            cards,
            definitions,
        } = super::catalogue::materialize(
            &self.inventory,
            &self.registry,
            &self.inputs.definitions,
            &self.inputs.decks,
            &self.inputs.inventory,
            ["W", "G"],
        )?;
        let mut fixture = Fixture {
            case: CaseId::Teaching(case),
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
        // Native opening shuffles follow actual starting-player order while
        // the game retains its canonical participant order.
        for player in [
            seat(&recipe.starting_role)?,
            other(seat(&recipe.starting_role)?)?,
        ] {
            let before = fixture
                .setup
                .seed
                .cards
                .iter()
                .filter(|v| v.owner == player)
                .map(|v| v.card)
                .collect();
            let target = recipe
                .shuffle_target_orders_top_first
                .get(role(player)?)
                .ok_or("initial shuffle recipe missing")?;
            fixture.add_shuffle(
                format!("{}.{}.initial", case.as_str(), role(player)?),
                player,
                before,
                target,
            )?;
        }
        for shuffle in &recipe.mulligan_shuffles {
            if shuffle.taken_after_shuffle != 1 {
                return Err("unsupported authored mulligan round".into());
            }
            let initial = recipe
                .shuffle_target_orders_top_first
                .get(&shuffle.role)
                .ok_or("mulligan initial order missing")?;
            let before = initial[7..]
                .iter()
                .chain(&initial[..7])
                .map(|a| fixture.card(a))
                .collect::<Result<Vec<_>, _>>()?;
            fixture.add_shuffle(
                format!("{}.{}.mulligan.1", case.as_str(), shuffle.role),
                seat(&shuffle.role)?,
                before,
                &shuffle.target_order_top_first,
            )?;
        }
        fixture.setup.entropy = EntropyTape::new(
            fixture
                .shuffles
                .iter()
                .flat_map(|v| v.words.iter().copied())
                .collect(),
        );
        Ok(fixture)
    }
    /// Build twenty library/opening inputs for the selected ordinary lifecycles.
    ///
    /// # Errors
    /// Rejects missing guarded definitions, malformed library permutations and
    /// context mismatches. All physical seeds still come from the actual registry.
    pub fn lifecycle_inputs(
        &self,
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<Vec<LifecycleInput>, String> {
        let mut result = vec![];
        let mut land_index = 0;
        let mut creature_index = 0;
        for (index, definition) in self.inputs.definitions.iter().enumerate() {
            let player = if self.inputs.decks[0]
                .entries
                .iter()
                .any(|e| e.oracle_id == definition.oracle_id)
            {
                A
            } else {
                B
            };
            let own_role = role(player)?;
            let target_alias = format!("{own_role}.{}.1", slug(&definition.name));
            let mut fixture = self.fixture(TeachingCase::Rd01, vocabulary.clone())?;
            let target = fixture.card(&target_alias)?;
            let program = fixture
                .setup
                .seed
                .cards
                .iter()
                .find(|v| v.card == target)
                .ok_or("lifecycle target seed absent")?
                .program
                .clone();
            let is_land = program.has_type(spellwake_engine::program::CardType::Land);
            let id = if is_land {
                land_index += 1;
                format!("TD-L{land_index:02}")
            } else {
                creature_index += 1;
                format!("TD-C{creature_index:02}")
            };
            fixture.setup.seed.active = player;
            fixture.shuffles.clear();
            for owner in [player, other(player)?] {
                let basic = if owner == A { "plains" } else { "forest" };
                let mut order = vec![];
                if owner == player {
                    order.push(target_alias.clone());
                }
                for n in 1..=7 {
                    let alias = format!("{}.{}.{n}", role(owner)?, basic);
                    if !order.contains(&alias) {
                        order.push(alias);
                    }
                }
                for seed in &fixture.setup.seed.cards {
                    if seed.owner == owner {
                        let alias = fixture.alias(seed.card)?.alias.clone();
                        if !order.contains(&alias) {
                            order.push(alias);
                        }
                    }
                }
                let before = fixture
                    .setup
                    .seed
                    .cards
                    .iter()
                    .filter(|v| v.owner == owner)
                    .map(|v| v.card)
                    .collect();
                fixture.add_shuffle(
                    format!("{id}.{}.initial", role(owner)?),
                    owner,
                    before,
                    &order,
                )?;
            }
            fixture.setup.entropy = EntropyTape::new(
                fixture
                    .shuffles
                    .iter()
                    .flat_map(|v| v.words.iter().copied())
                    .collect(),
            );
            let context = GameContext::new(
                fixture.context_inputs.release.clone(),
                fixture.context_inputs.rules.clone(),
                fixture.context_inputs.catalogue.clone(),
                fixture.context_inputs.programs.clone(),
                format!("modern-bo1-teaching-lifecycle-v1:{id}"),
                vocabulary.id(),
                fixture.setup.seed.name_rules.id(),
            )
            .map_err(|e| format!("lifecycle context: {e:?}"))?;
            let lands = (1..=7)
                .map(|n| {
                    fixture.card(&format!(
                        "{own_role}.{}.{n}",
                        if player == A { "plains" } else { "forest" }
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?;
            result.push(LifecycleInput {
                id,
                name: definition.name.clone(),
                oracle_id: definition.oracle_id.clone(),
                game: GameId(7000 + index as u128),
                context,
                setup: fixture.setup,
                target,
                owner: player,
                lands,
                program,
            });
        }
        Ok(result)
    }
    /// Execute selected ordinary trajectories using the shared recorder/reexecutor.
    ///
    /// # Errors
    /// Rejects an empty case selection or failed source/context materialization.
    /// Gameplay failures retain their actual evidence inside incomplete RunReports.
    pub fn run(
        &self,
        cases: &[TeachingCase],
        vocabulary: Arc<CreatureTypeVocabulary>,
    ) -> Result<TeachingReport, String> {
        if cases.is_empty() {
            return Err("teaching suite requires at least one case".into());
        }
        let mut runs = vec![];
        for (index, case) in cases.iter().enumerate() {
            runs.push(record::run_fixture_at(
                self.fixture(*case, vocabulary.clone())?,
                index,
            ));
        }
        Ok(TeachingReport {
            schema_version: 1,
            profile: "catalogue-teaching-pair-v1",
            registry_version: TEACHING_REGISTRY_VERSION,
            catalogue_sha256: self.inventory.compressed_sha256.clone(),
            runs,
        })
    }
}
/// Guarded library-only setup for one native ordinary lifecycle.
#[derive(Debug, Clone)]
pub struct LifecycleInput {
    pub id: String,
    pub name: String,
    pub oracle_id: String,
    pub game: GameId,
    pub context: GameContext,
    pub setup: OpeningSetup,
    pub target: CardId,
    pub owner: SeatId,
    pub lands: Vec<CardId>,
    pub program: spellwake_engine::effects::Program,
}
/// Privileged local evidence; complete-card/global-coverage claims are separate.
#[derive(Debug, Serialize)]
pub struct TeachingReport {
    schema_version: u32,
    profile: &'static str,
    registry_version: u32,
    catalogue_sha256: String,
    pub runs: Vec<RunReport>,
}
impl TeachingReport {
    /// Whether every selected trajectory completed its ordinary/reexecution checks.
    pub fn completed(&self) -> bool {
        !self.runs.is_empty() && self.runs.iter().all(RunReport::completed)
    }
}

#[derive(Debug, Clone)]
pub(super) enum Intent {
    Action(usize),
    Declare(usize),
    Bottom(usize),
    Attack(usize),
    Discard(usize),
    Order(usize),
    Pass,
    EmptyAttack,
    Block(usize),
    EmptyBlock,
}
#[derive(Debug, Clone)]
pub(super) struct TeachingPolicy {
    fixture: Fixture,
    recipe: CaseInput,
    consumed: BTreeSet<String>,
}
impl TeachingPolicy {
    pub fn new(fixture: &Fixture) -> Result<Option<Self>, String> {
        let CaseId::Teaching(case) = fixture.case else {
            return Ok(None);
        };
        let recipe = inputs()?
            .cases
            .into_iter()
            .find(|v| v.id == case.as_str())
            .ok_or("teaching policy input absent")?;
        Ok(Some(Self {
            fixture: fixture.clone(),
            recipe,
            consumed: BTreeSet::new(),
        }))
    }
    pub(super) fn accept(&mut self, intent: Intent) -> Result<(), String> {
        let key = match intent {
            Intent::Action(i) => Some(format!("action.{i}")),
            Intent::Declare(i) => Some(format!("declare.{i}")),
            Intent::Bottom(i) => Some(format!("bottom.{i}")),
            Intent::Attack(i) => Some(format!("attack.{i}")),
            Intent::Discard(i) => Some(format!("discard.{i}")),
            Intent::Order(i) => Some(format!("order.{i}")),
            Intent::Block(i) => Some(format!("block.{i}")),
            _ => None,
        };
        if let Some(key) = key
            && !self.consumed.insert(key)
        {
            return Err("teaching intention consumed twice".into());
        }
        Ok(())
    }
    pub fn finish(&self, state: &GameState) -> Result<(), String> {
        for (kind, count) in [
            ("action", self.recipe.actions.len()),
            ("declare", self.recipe.opening_declarations.len()),
            ("bottom", self.recipe.london_bottom.len()),
            ("attack", self.recipe.combat.len()),
            ("discard", self.recipe.cleanup_discards.len()),
            ("order", self.recipe.state_check_order_choices.len()),
        ] {
            for i in 0..count {
                if !self.consumed.contains(&format!("{kind}.{i}")) {
                    return Err(format!("unconsumed teaching {kind}.{i}"));
                }
            }
        }
        for (i, c) in self.recipe.combat.iter().enumerate() {
            if !c.blockers.is_empty() && !self.consumed.contains(&format!("block.{i}")) {
                return Err(format!("unconsumed teaching block.{i}"));
            }
        }
        if state.outcome().is_none()
            || state.turn().is_none_or(|t| t.position().number != 13)
            || state.entropy().is_none_or(|t| t.remaining() != 0)
        {
            return Err("teaching terminal/entropy/issued boundary mismatch".into());
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
        if actual.iter().copied().collect::<BTreeSet<_>>() != expected
            || actual.len() != expected.len()
        {
            return Err(format!(
                "teaching offered domain differs: expected {aliases:?}, actual {actual:?}"
            ));
        }
        Ok(())
    }
    fn combat(&self, state: &GameState) -> Option<(usize, &Combat)> {
        let turn = state.turn()?.position();
        let active = role(state.execution().window().active_player()).ok()?;
        self.recipe
            .combat
            .iter()
            .enumerate()
            .find(|(_, c)| c.active_role == active && c.own_turn == turn.number.div_ceil(2))
    }
    pub(super) fn prepare(&self, state: &GameState) -> Result<(GameInput, Intent), String> {
        if state.basis().game != self.fixture.game
            || state.basis().context != self.fixture.context
            || state.outcome().is_some()
        {
            return Err("teaching game/context/outcome mismatch".into());
        }
        if state.turn().is_some_and(|t| t.position().number > 13) {
            return Err("teaching turn watchdog exhausted".into());
        }
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
                                && !self.consumed.contains(&format!("declare.{i}"))
                        })
                        .ok_or("unexpected teaching opening declaration")?;
                    let choice = match d.choice.as_str() {
                        "keep" => MulliganChoice::Keep,
                        "mulligan" => MulliganChoice::Mulligan,
                        _ => return Err("unknown teaching declaration".into()),
                    };
                    (DecisionReply::Mulligan(choice), Intent::Declare(i))
                }
                RuleFrame::Opening(OpeningDecision::Bottom {
                    player,
                    count,
                    hand,
                }) => {
                    let (i, b) = self
                        .recipe
                        .london_bottom
                        .iter()
                        .enumerate()
                        .find(|(i, b)| {
                            seat(&b.role).ok() == Some(*player)
                                && !self.consumed.contains(&format!("bottom.{i}"))
                        })
                        .ok_or("unexpected teaching bottom choice")?;
                    if *count != b.count {
                        return Err("teaching bottom count mismatch".into());
                    }
                    self.domain(state, hand, &b.hand_domain)?;
                    (
                        DecisionReply::Bottom(self.objects(state, &b.selected_top_first)?),
                        Intent::Bottom(i),
                    )
                }
                RuleFrame::Combat(CombatDecision::Attackers(frame)) => {
                    let selected = self.combat(state);
                    let aliases = selected
                        .map(|(_, c)| c.attackers.clone())
                        .unwrap_or_default();
                    let mut eligible = vec![];
                    let turn = state
                        .turn()
                        .ok_or("attacker without turn")?
                        .position()
                        .number
                        .div_ceil(2);
                    let r = role(actor)?;
                    for action in &self.recipe.actions {
                        if action.kind == "cast_creature"
                            && action.role == r
                            && action.own_turn < turn
                        {
                            let id = self.fixture.current_object(state, &action.card)?;
                            if state.execution().window().cards().get(id).is_some_and(|o| {
                                o.zone() == ObjectZone::Battlefield && o.tapped() == Some(false)
                            }) {
                                eligible.push(action.card.clone());
                            }
                        }
                    }
                    self.domain(state, frame.eligible_attackers(), &eligible)?;
                    if frame.defenders() != [other(actor)?] {
                        return Err("teaching defender domain mismatch".into());
                    }
                    let attacks = self
                        .objects(state, &aliases)?
                        .into_iter()
                        .map(|attacker| AttackDeclaration {
                            attacker,
                            defender: other(actor).expect("known teaching seat"),
                            defending_planeswalker: None,
                        })
                        .collect();
                    (
                        DecisionReply::DeclareAttackers(attacks),
                        selected
                            .map(|(i, _)| Intent::Attack(i))
                            .unwrap_or(Intent::EmptyAttack),
                    )
                }
                RuleFrame::Combat(CombatDecision::Blockers(frame)) => {
                    let (combat_index, combat) = self
                        .combat(state)
                        .ok_or("unexpected teaching blocker frame")?;
                    if seat(&combat.defender_role)? != actor {
                        return Err("teaching blocker chooser mismatch".into());
                    }
                    self.domain(state, frame.attackers(), &combat.attackers)?;
                    let eligible = self
                        .recipe
                        .actions
                        .iter()
                        .filter(|a| a.kind == "cast_creature" && seat(&a.role).ok() == Some(actor))
                        .filter_map(|a| {
                            self.fixture
                                .current_object(state, &a.card)
                                .ok()
                                .map(|id| (a, id))
                        })
                        .filter(|(_, id)| {
                            state
                                .execution()
                                .window()
                                .cards()
                                .get(*id)
                                .is_some_and(|o| {
                                    o.zone() == ObjectZone::Battlefield && o.tapped() == Some(false)
                                })
                        })
                        .map(|(a, _)| a.card.clone())
                        .collect::<Vec<_>>();
                    self.domain(state, frame.eligible_blockers(), &eligible)?;
                    let blocks = combat
                        .blockers
                        .iter()
                        .map(|b| {
                            Ok(BlockDeclaration {
                                blocker: self.fixture.current_object(state, &b.blocker)?,
                                attacker: self.fixture.current_object(state, &b.attacker)?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    (
                        DecisionReply::DeclareBlockers(blocks),
                        if combat.blockers.is_empty() {
                            Intent::EmptyBlock
                        } else {
                            Intent::Block(combat_index)
                        },
                    )
                }
                RuleFrame::Combat(CombatDecision::DamageAssignments(_)) => {
                    return Err(
                        "teaching single-recipient assignments must be forced by the actual engine"
                            .into(),
                    );
                }
                RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) => {
                    let (i, o) = self
                        .recipe
                        .state_check_order_choices
                        .iter()
                        .enumerate()
                        .find(|(i, o)| {
                            seat(&o.owner_role).ok() == Some(actor)
                                && !self.consumed.contains(&format!("order.{i}"))
                        })
                        .ok_or("unexpected teaching graveyard-order choice")?;
                    if o.at != "G4.after_combat_damage"
                        || state.turn().is_none_or(|t| t.position().number != 7)
                    {
                        return Err("teaching order boundary mismatch".into());
                    }
                    self.domain(state, frame.domain(), &o.domain)?;
                    (
                        DecisionReply::OrderGraveyard(
                            self.objects(state, &o.selected_graveyard_top_first)?,
                        ),
                        Intent::Order(i),
                    )
                }
                RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) => {
                    let turn = state.turn().ok_or("discard without turn")?.position();
                    let (i, d) = self
                        .recipe
                        .cleanup_discards
                        .iter()
                        .enumerate()
                        .find(|(i, d)| {
                            seat(&d.role).ok() == Some(actor)
                                && d.own_turn == turn.number.div_ceil(2)
                                && !self.consumed.contains(&format!("discard.{i}"))
                        })
                        .ok_or("unexpected teaching discard choice")?;
                    if turn.step != Step::Cleanup
                        || frame.domain().len() != d.hand_count_before
                        || frame.required() != d.cards.len()
                        || d.hand_count_before - d.cards.len() != d.hand_count_after
                    {
                        return Err("teaching discard domain/count mismatch".into());
                    }
                    (
                        DecisionReply::Discard(self.objects(state, &d.cards)?),
                        Intent::Discard(i),
                    )
                }
                _ => return Err("unexpected teaching decision family".into()),
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
                return Err("teaching expected actual priority or decision".into());
            };
            let turn = state
                .turn()
                .ok_or("teaching priority without turn")?
                .position();
            let active = state.execution().window().active_player();
            let next = self.recipe.actions.iter().enumerate().find(|(i, a)| {
                !self.consumed.contains(&format!("action.{i}"))
                    && seat(&a.role).ok() == Some(active)
                    && a.own_turn == turn.number.div_ceil(2)
            });
            if holder == active
                && turn.step == Step::PrecombatMain
                && state.execution().window().entries().is_empty()
            {
                if let Some((i, a)) = next {
                    if a.step != "precombat_main" || !a.targets.is_empty() {
                        return Err("teaching action shape unsupported".into());
                    }
                    let object = self.fixture.current_object(state, &a.card)?;
                    let command = match a.kind.as_str() {
                        "play_land" => GameCommand::PlayLand { land: object },
                        "activate_intrinsic_mana" => GameCommand::ActivateMana {
                            source: object,
                            ability: ManaAbilityRef::BasicLand(if active == A {
                                BasicLandType::Plains
                            } else {
                                BasicLandType::Forest
                            }),
                        },
                        "cast_creature" => {
                            if a.required_cost.as_ref().is_none_or(String::is_empty) {
                                return Err("teaching cast cost recipe missing".into());
                            }
                            let units = a
                                .mana_units_paid
                                .as_ref()
                                .ok_or("teaching payment recipe missing")?
                                .parse::<u32>()
                                .map_err(|_| "invalid teaching mana amount")?;
                            GameCommand::CastUntargeted {
                                spell: object,
                                payment: Some(ManaPayment {
                                    from_lots: Default::default(),
                                    spend: vec![ManaSpend {
                                        mana_type: if active == A {
                                            ManaType::White
                                        } else {
                                            ManaType::Green
                                        },
                                        amount: i64::from(units).into(),
                                    }],
                                }),
                            }
                        }
                        _ => return Err("unknown teaching action kind".into()),
                    };
                    (holder, command, Intent::Action(i))
                } else {
                    (holder, GameCommand::Pass, Intent::Pass)
                }
            } else {
                (holder, GameCommand::Pass, Intent::Pass)
            }
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

pub(super) fn reached_probes(
    fixture: &Fixture,
    state: &GameState,
    prepared: Option<&super::policy::PreparedInput>,
) -> Result<Vec<RejectionProbe>, String> {
    use spellwake_engine::game::GameError;
    let mut probes = vec![];
    if state.outcome().is_some() {
        return Ok(probes);
    }
    let valid = &prepared
        .ok_or("nonterminal reached probes require the one prepared input")?
        .input;
    let mut add = |id: &str, input: GameInput, expected: GameError| {
        probes.push(RejectionProbe {
            id: format!("{}.{}", fixture.case.as_str(), id),
            expectation_ref: id.into(),
            attachment: "actual-teaching-reached-state".into(),
            input,
            expected,
        });
    };
    if state
        .turn()
        .is_some_and(|t| t.position().number == 1 && t.position().step == Step::Upkeep)
    {
        let mut input = valid.clone();
        input.basis.revision = input.basis.revision.saturating_sub(1);
        add("stale", input, GameError::StaleRevision);
        let mut input = valid.clone();
        input.actor = SeatId(999);
        add("foreign", input, GameError::UnknownActor);
    }
    if let GameCommand::ActivateMana { .. } = valid.command {
        let own = role(valid.actor)?;
        let n = state
            .turn()
            .ok_or("mana without turn")?
            .position()
            .number
            .div_ceil(2);
        if n == 1 || own == "G" && n == 2 {
            let recipe = inputs()?
                .cases
                .into_iter()
                .find(|c| c.id == fixture.case.as_str())
                .ok_or("case missing")?;
            if let Some(cast) = recipe
                .actions
                .iter()
                .find(|a| a.kind == "cast_creature" && a.role == own && a.own_turn == n)
            {
                let amount = cast
                    .mana_units_paid
                    .as_ref()
                    .ok_or("cast amount absent")?
                    .parse::<u32>()
                    .map_err(|_| "invalid amount")?;
                let available = state.execution().mana(
                    valid.actor,
                    if own == "W" {
                        ManaType::White
                    } else {
                        ManaType::Green
                    },
                );
                if available == 0.into() {
                    let mut input = valid.clone();
                    input.command = GameCommand::CastUntargeted {
                        spell: fixture.current_object(state, &cast.card)?,
                        payment: Some(ManaPayment {
                            from_lots: Default::default(),
                            spend: vec![ManaSpend {
                                mana_type: if own == "W" {
                                    ManaType::White
                                } else {
                                    ManaType::Green
                                },
                                amount: i64::from(amount).into(),
                            }],
                        }),
                    };
                    add(
                        "insufficient-payment",
                        input,
                        GameError::Effect(EffectError::InsufficientMana),
                    );
                }
            }
        }
    }
    if state
        .turn()
        .is_some_and(|t| t.position().number == 1 && t.position().step == Step::PrecombatMain)
        && state.pending_decision().is_none()
        && state.execution().window().entries().is_empty()
    {
        let active = state.execution().window().active_player();
        if valid.actor == active {
            let base = if active == A { "W.plains" } else { "G.forest" };
            let first = fixture.current_object(state, &format!("{base}.1"))?;
            if state
                .execution()
                .window()
                .cards()
                .get(first)
                .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
            {
                let mut input = valid.clone();
                input.command = GameCommand::PlayLand {
                    land: fixture.current_object(state, &format!("{base}.2"))?,
                };
                add(
                    "second-land",
                    input,
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
        && fixture.case == CaseId::Teaching(TeachingCase::Rd01)
        && state.turn().is_some_and(|t| t.position().number == 3)
    {
        let mut input = valid.clone();
        input.command = GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: fixture.current_object(state, "W.elite-vanguard.1")?,
                defender: B,
                defending_planeswalker: None,
            }]),
        };
        add(
            "young-attacker",
            input,
            GameError::Effect(EffectError::Combat(CombatError::IllegalAttacker)),
        );
    }
    Ok(probes)
}
