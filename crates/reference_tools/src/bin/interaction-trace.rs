//! Native observations for restricted interaction profiles, not a full-game benchmark.
#[path = "interaction-trace/copies.rs"]
mod copies;
use serde::Serialize;
use spellwake_card_programs::{BOLT, COUNTERSPELL, LION, OracleId, Registry, UNSUMMON};
use spellwake_content::snapshot::{SnapshotError, decode, load};
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    effects::{CopyTargetChoice, EffectError, EffectWindow, SeedCard},
    game::{GameCommand, GameContext, GameError, GameId, GameInput, GameState},
    mana::{FloatingMana, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
};
use std::{collections::BTreeMap, path::Path, process::ExitCode};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const NATIVE_DRIVER: &str = "game-state-initialize-advance-v1";

#[derive(Serialize)]
struct CardView {
    alias: &'static str,
    owner: &'static str,
    zone: &'static str,
    damage: Option<RulesInteger>,
    power: Option<RulesInteger>,
    toughness: Option<RulesInteger>,
}
#[derive(Serialize)]
struct EntryView {
    card: &'static str,
    targets: Vec<&'static str>,
}
#[derive(Serialize)]
struct Observation {
    checkpoint: &'static str,
    stack_bottom_first: Vec<EntryView>,
    cards: Vec<CardView>,
}
#[derive(Serialize)]
struct Scenario {
    scenario: &'static str,
    observations: Vec<Observation>,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    kind: &'static str,
    scope: &'static str,
    full_game_comparisons: u32,
    fully_qualified_cards: u32,
    scenarios: Vec<Scenario>,
    registry_input: &'static str,
    registry_source_sha256: String,
    native_driver: &'static str,
}
fn alias(card: CardId) -> Result<&'static str, EffectError> {
    match card.0 {
        1 => Ok("bolt"),
        2 => Ok("bounce"),
        3 => Ok("lion"),
        4 => Ok("counter"),
        5 => Ok("rescue"),
        _ => Err(EffectError::InconsistentProgram),
    }
}
fn identity(state: &EffectWindow, card: u64) -> Result<CardObjectId, EffectError> {
    state
        .window()
        .cards()
        .current_identity(CardId(card))
        .ok_or(EffectError::UnknownCard)
}
#[derive(Default)]
struct Aliases {
    cards: BTreeMap<CardObjectId, &'static str>,
    stack: BTreeMap<StackEntryId, &'static str>,
}
fn observe(
    state: &EffectWindow,
    checkpoint: &'static str,
    aliases: &mut Aliases,
) -> Result<Observation, EffectError> {
    for card in state.window().cards().iter() {
        aliases.cards.insert(card.id(), alias(card.card())?);
    }
    for entry in state.window().entries() {
        let StackKind::CardSpell(object) = entry.kind() else {
            return Err(EffectError::InconsistentProgram);
        };
        aliases.stack.insert(
            entry.id(),
            *aliases
                .cards
                .get(object)
                .ok_or(EffectError::InconsistentProgram)?,
        );
    }
    let mut stack = Vec::new();
    for entry in state.window().entries() {
        let card = *aliases
            .stack
            .get(&entry.id())
            .ok_or(EffectError::InconsistentProgram)?;
        let mut targets = Vec::new();
        for target in entry.targets().targets() {
            let name = match target.choice().target {
                TargetRef::Card(object) => aliases.cards.get(&object),
                TargetRef::Stack(id) => aliases.stack.get(&id),
                TargetRef::Player(_) => None,
            }
            .ok_or(EffectError::InconsistentProgram)?;
            targets.push(*name);
        }
        stack.push(EntryView { card, targets });
    }
    let mut cards = Vec::new();
    for key in 1..=5 {
        let Some(object) = state.window().cards().current_identity(CardId(key)) else {
            continue;
        };
        let card = state
            .window()
            .cards()
            .get(object)
            .ok_or(EffectError::InconsistentProgram)?;
        let creature = state.creature(object);
        cards.push(CardView {
            alias: alias(card.card())?,
            owner: if card.owner() == A { "A" } else { "B" },
            zone: match card.zone() {
                CardZone::Hand(_) => "HAND",
                CardZone::Stack => "STACK",
                CardZone::Battlefield => "BATTLEFIELD",
                CardZone::Graveyard(_) => "GRAVEYARD",
                _ => return Err(EffectError::InconsistentProgram),
            },
            damage: creature.as_ref().map(|view| view.damage.clone()),
            power: creature.as_ref().map(|view| view.power.clone()),
            toughness: creature.map(|view| view.toughness),
        });
    }
    Ok(Observation {
        checkpoint,
        stack_bottom_first: stack,
        cards,
    })
}
fn initialize(
    registry: &Registry,
    game: GameId,
    profile: &str,
    cards: Vec<SeedCard>,
    mana: Vec<FloatingMana>,
) -> Result<GameState, GameError> {
    // These are trusted native source identities, not claims of rules qualification.
    let context = GameContext::new(
        "native-core-first-slice-v1".into(),
        "magic-cr-20260925-restricted-instants-v1".into(),
        format!(
            "oracle-snapshot-sha256:{}",
            registry.coverage().source_sha256
        ),
        format!(
            "guarded-native-registry-v{}",
            registry.coverage().registry_version
        ),
        profile.into(),
    )?;
    Ok(GameState::initialize(game, context, vec![A, B], A, cards, mana)?.into_state())
}
fn advance(state: GameState, actor: SeatId, command: GameCommand) -> Result<GameState, GameError> {
    Ok(state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        })?
        .into_state())
}
fn pass(state: GameState, actor: SeatId) -> Result<GameState, GameError> {
    advance(state, actor, GameCommand::Pass)
}
fn holding(mut state: GameState, actor: SeatId) -> Result<GameState, GameError> {
    for _ in 0..state.execution().window().participants().len() {
        if state.pending_decision().is_some() {
            return Err(GameError::DecisionRequired);
        }
        match state.execution().window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == actor => {
                return Ok(state);
            }
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => {
                state = pass(state, holder)?;
            }
            _ => return Err(GameError::UnexpectedKernelPhase),
        }
    }
    Err(GameError::UnexpectedKernelPhase)
}
fn cast(
    state: GameState,
    actor: SeatId,
    card: u64,
    target: TargetRef,
) -> Result<GameState, GameError> {
    let state = holding(state, actor)?;
    let spell = identity(state.execution(), card)?;
    advance(state, actor, GameCommand::Cast { spell, target })
}
fn resolve(
    mut state: GameState,
    mut copy: Option<CopyTargetChoice>,
) -> Result<GameState, GameError> {
    let entry = state
        .execution()
        .window()
        .entries()
        .last()
        .ok_or(GameError::UnexpectedKernelPhase)?
        .id();
    // Resolve only the entry selected at invocation, including its suspended reply.
    for _ in 0..=state.execution().window().participants().len() {
        state = if let Some(pending) = state.pending_decision() {
            let actor = pending.chooser();
            let decision = pending.id();
            let choice = copy.take().ok_or(GameError::DecisionRequired)?;
            advance(
                state,
                actor,
                GameCommand::Reply {
                    decision,
                    choice: choice.into(),
                },
            )?
        } else {
            match state.execution().window().phase() {
                WindowPhase::Priority(PriorityPhase::Ready { holder }) => pass(state, holder)?,
                _ => return Err(GameError::UnexpectedKernelPhase),
            }
        };
        if state
            .execution()
            .window()
            .entries()
            .iter()
            .all(|current| current.id() != entry)
        {
            return Ok(state);
        }
    }
    Err(GameError::UnexpectedKernelPhase)
}
fn registered_seed(
    registry: &Registry,
    oracle: &str,
    card: u64,
    owner: SeatId,
    destination: CardDestination,
) -> Result<spellwake_engine::effects::SeedCard, EffectError> {
    let id = OracleId::try_from(oracle.to_owned()).map_err(|_| EffectError::InconsistentProgram)?;
    registry
        .seed(&id, CardId(card), owner, destination)
        .map_err(|_| EffectError::InconsistentProgram)
}
fn bounce_scenario(bounce: bool, registry: &Registry) -> Result<Scenario, GameError> {
    let mut cards = vec![
        registered_seed(registry, BOLT, 1, A, CardDestination::Hand)?,
        registered_seed(
            registry,
            LION,
            3,
            B,
            CardDestination::Battlefield { controller: B },
        )?,
    ];
    if bounce {
        cards.push(registered_seed(
            registry,
            UNSUMMON,
            2,
            B,
            CardDestination::Hand,
        )?);
    }
    let mut state = initialize(
        registry,
        GameId(if bounce { 2 } else { 1 }),
        "restricted-instant-interactions-v2",
        cards,
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: 1.into(),
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Blue,
                amount: 1.into(),
            },
        ],
    )?;
    let mut aliases = Aliases::default();
    // Preserve starting incarnation aliases before casts/zone moves renew identities.
    for card in state.execution().window().cards().iter() {
        aliases.cards.insert(card.id(), alias(card.card())?);
    }
    let lion = TargetRef::Card(identity(state.execution(), 3)?);
    state = cast(state, A, 1, lion)?;
    let mut observations = vec![observe(state.execution(), "bolt-on-stack", &mut aliases)?];
    if bounce {
        state = cast(state, B, 2, lion)?;
        observations.push(observe(
            state.execution(),
            "response-on-stack",
            &mut aliases,
        )?);
        state = resolve(state, None)?;
        observations.push(observe(state.execution(), "after-response", &mut aliases)?);
    }
    state = resolve(state, None)?;
    observations.push(observe(state.execution(), "after-bolt", &mut aliases)?);
    Ok(Scenario {
        scenario: if bounce {
            "target-bounce"
        } else {
            "bolt-control"
        },
        observations,
    })
}
fn stack_target(state: &EffectWindow, card: u64) -> Result<TargetRef, EffectError> {
    state.window().entries().iter().find(|entry| matches!(entry.kind(), StackKind::CardSpell(object) if state.window().cards().get(*object).is_some_and(|object| object.card() == CardId(card))))
        .map(|entry| TargetRef::Stack(entry.id())).ok_or(EffectError::InconsistentProgram)
}
fn counter_scenario(rescue: bool, registry: &Registry) -> Result<Scenario, GameError> {
    let mut cards = vec![
        registered_seed(registry, BOLT, 1, A, CardDestination::Hand)?,
        registered_seed(
            registry,
            LION,
            3,
            B,
            CardDestination::Battlefield { controller: B },
        )?,
        registered_seed(registry, COUNTERSPELL, 4, B, CardDestination::Hand)?,
    ];
    if rescue {
        cards.push(registered_seed(
            registry,
            COUNTERSPELL,
            5,
            A,
            CardDestination::Hand,
        )?);
    }
    let mut state = initialize(
        registry,
        GameId(if rescue { 4 } else { 3 }),
        "restricted-instant-interactions-v2",
        cards,
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: 1.into(),
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Blue,
                amount: 2.into(),
            },
            FloatingMana {
                player: A,
                mana_type: ManaType::Blue,
                amount: 2.into(),
            },
        ],
    )?;
    let mut aliases = Aliases::default();
    let lion = TargetRef::Card(identity(state.execution(), 3)?);
    state = cast(state, A, 1, lion)?;
    let mut observations = vec![observe(state.execution(), "bolt-on-stack", &mut aliases)?];
    let bolt = stack_target(state.execution(), 1)?;
    state = cast(state, B, 4, bolt)?;
    observations.push(observe(
        state.execution(),
        "counter-on-stack",
        &mut aliases,
    )?);
    if rescue {
        let counter = stack_target(state.execution(), 4)?;
        state = cast(state, A, 5, counter)?;
        observations.push(observe(state.execution(), "rescue-on-stack", &mut aliases)?);
        state = resolve(state, None)?;
        observations.push(observe(state.execution(), "after-rescue", &mut aliases)?);
        state = resolve(state, None)?;
        observations.push(observe(state.execution(), "after-bolt", &mut aliases)?);
    } else {
        state = resolve(state, None)?;
        observations.push(observe(state.execution(), "after-counter", &mut aliases)?);
    }
    Ok(Scenario {
        scenario: if rescue {
            "counter-counter"
        } else {
            "counter-bolt"
        },
        observations,
    })
}
fn run(registry: &Registry, input: &'static str) -> Result<Report, GameError> {
    Ok(Report {
        schema_version: 2,
        kind: "restricted-instant-interactions",
        scope: "Guarded Oracle registry programs; post-cast damage/bounce/counter stack/target/zone/PT/damage checkpoints. Seeded floating mana; no land activation, turn traversal, life/player targeting, full-card or full-game qualification.",
        full_game_comparisons: 0,
        fully_qualified_cards: 0,
        scenarios: vec![
            bounce_scenario(false, registry)?,
            bounce_scenario(true, registry)?,
            counter_scenario(false, registry)?,
            counter_scenario(true, registry)?,
        ],
        registry_input: input,
        registry_source_sha256: registry.coverage().source_sha256.clone(),
        native_driver: NATIVE_DRIVER,
    })
}
fn read_registry(copy_mode: bool) -> Result<(Registry, &'static str), SnapshotError> {
    let args: Vec<_> = std::env::args()
        .skip(if copy_mode { 2 } else { 1 })
        .collect();
    let (inventory, input) = match args.as_slice() {
        [] if copy_mode => (
            decode(
                include_bytes!("../../../../tests/catalogue/spell-copy-cards.meta.json"),
                include_bytes!("../../../../tests/catalogue/spell-copy-cards.jsonl.gz").to_vec(),
            )?,
            "embedded-five-card-test-subset",
        ),
        [] => (
            decode(
                include_bytes!("../../../../tests/catalogue/registry-cards.meta.json"),
                include_bytes!("../../../../tests/catalogue/registry-cards.jsonl.gz").to_vec(),
            )?,
            "embedded-four-card-test-subset",
        ),
        [metadata, snapshot] => (
            load(Path::new(metadata), Path::new(snapshot))?,
            "saved-snapshot",
        ),
        _ => {
            return Err(SnapshotError::Invalid {
                field: "arguments",
                message: "expected no arguments (embedded test subset) or metadata/snapshot paths"
                    .into(),
            });
        }
    };
    let registry =
        Registry::from_inventory(&inventory).map_err(|error| SnapshotError::Invalid {
            field: "registry",
            message: error.to_string(),
        })?;
    let mut required = vec![BOLT, UNSUMMON, LION, COUNTERSPELL];
    if copy_mode {
        required.push(spellwake_card_programs::TWINCAST);
    }
    for required in required {
        let id =
            OracleId::try_from(required.to_owned()).map_err(|error| SnapshotError::Invalid {
                field: "registry",
                message: error.to_string(),
            })?;
        if registry.resolve(&id).is_none() {
            return Err(SnapshotError::Invalid {
                field: "registry",
                message: "required program is missing or blocked by its semantic guard".into(),
            });
        }
    }
    Ok((registry, input))
}
fn main() -> ExitCode {
    let copy_mode = std::env::args().nth(1).as_deref() == Some("--copies");
    let (registry, input) = match read_registry(copy_mode) {
        Ok(value) => value,
        Err(error) => {
            let response: ApiResponse<()> = match error {
                SnapshotError::Invalid { field, message } => {
                    eprintln!("invalid native source: {message}");
                    ApiResponse::Fail {
                        data: BTreeMap::from([(field.into(), "invalid-input".into())]),
                    }
                }
                SnapshotError::Operational(error) => {
                    eprintln!("native source unavailable: {error}");
                    ApiResponse::Error {
                        message: "Native source unavailable".into(),
                        code: "native-source-unavailable".into(),
                    }
                }
            };
            if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
                eprintln!("native trace output failed: {error}");
            }
            return ExitCode::FAILURE;
        }
    };
    #[derive(Serialize)]
    #[serde(untagged)]
    enum ProfileReport {
        Instants(Report),
        Copies(copies::Report),
    }
    let result = if copy_mode {
        copies::run(&registry, input).map(ProfileReport::Copies)
    } else {
        run(&registry, input).map(ProfileReport::Instants)
    };
    let (response, success): (ApiResponse<ProfileReport>, bool) = match result {
        Ok(data) => (ApiResponse::Success { data }, true),
        Err(error) => {
            eprintln!("native interaction failed: {error:?}");
            (
                ApiResponse::Error {
                    message: "Native interaction unavailable".into(),
                    code: "native-interaction-error".into(),
                },
                false,
            )
        }
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
        eprintln!("native trace output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
