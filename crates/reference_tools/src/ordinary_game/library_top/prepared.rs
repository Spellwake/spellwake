//! Exact authored LT01–LT08 prepared consumer and privileged native diagnostics.
//! This neither reads reference results nor supplies an ordinary/report/public codec.
use super::{
    A, B, ISLAND, LibraryTopSource, LibraryTopSourceError, SCOUR,
    catalogue::{self, PhysicalRow, PreparedSetup, PreparedZone, Shape},
};
use crate::{packet, program, projection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spellwake_card_programs::{LIBRARY_TOP_PROFILE, TEACHING_SOURCE_SHA256};
use spellwake_contracts::names::NameRules;
use spellwake_engine::{
    combat::CombatRules,
    effects::{LibraryChoiceKind, LibraryTopReply},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::{collections::BTreeMap, sync::Arc};

mod diagnostic;
/// Exact input-only authored preparation; no reference execution data is included.
pub const AUTHORED_INPUT: &[u8] = include_bytes!("prepared/inputs-v1.json");
const INPUT_SHA256: &str = "b04d4954e89bcfb63551cd1e312bfb7bff3fa0885475c7882aa968b0dbf4dd27";
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Finite preparation failures preserve available native/source/IO causes.
#[derive(Debug)]
pub enum PreparedError {
    /// Original complete source4 boundary failure.
    Source(LibraryTopSourceError),
    /// Original source-row read failure.
    Io(std::io::Error),
    /// Original strict input schema failure.
    Json(serde_json::Error),
    /// Original native root failure; GameError has no std::error::Error implementation.
    Native(GameError),
    /// A closed admission, observation or replay invariant was rejected.
    Admission(String),
}
impl std::fmt::Display for PreparedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => write!(f, "prepared input schema: {e}"),
            Self::Native(e) => write!(f, "prepared native transition rejected: {e:?}"),
            Self::Admission(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for PreparedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}
impl From<String> for PreparedError {
    fn from(e: String) -> Self {
        Self::Admission(e)
    }
}
impl From<&str> for PreparedError {
    fn from(e: &str) -> Self {
        Self::Admission(e.into())
    }
}
impl From<GameError> for PreparedError {
    fn from(e: GameError) -> Self {
        Self::Native(e)
    }
}
fn require(ok: bool, message: &str) -> Result<(), PreparedError> {
    if ok { Ok(()) } else { Err(message.into()) }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionInput {
    name: String,
    oracle_id: String,
    snapshot_printing_id: String,
    edition: String,
    collector_number: String,
    source_row: Pin,
    script: Pin,
    edition_file: Pin,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    forge_commit: String,
    definitions: BTreeMap<String, DefinitionInput>,
    cases: Vec<Case>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Partition {
    actor: String,
    offered_aliases: Vec<String>,
    top_first: Vec<String>,
    other_top_first: Vec<String>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Case {
    case_id: String,
    source_definition: String,
    target_player: Option<String>,
    a_library_top_first: Vec<String>,
    b_library_top_first: Vec<String>,
    partition: Option<Partition>,
    graveyard_movement_order: Vec<String>,
}
fn parse(bytes: &[u8]) -> Result<Input, PreparedError> {
    require(
        digest(bytes) == INPUT_SHA256,
        "prepared input exact byte identity mismatch",
    )?;
    let value = packet::unique_json(bytes)?;
    serde_json::from_value(value).map_err(PreparedError::Json)
}
fn seat(role: &str) -> Result<SeatId, PreparedError> {
    match role {
        "A" => Ok(A),
        "B" => Ok(B),
        _ => Err("prepared foreign player role".into()),
    }
}
fn source_admission(source: &LibraryTopSource, input: &Input) -> Result<(), PreparedError> {
    source.validate().map_err(PreparedError::Source)?;
    require(
        input.schema == "library-top-forge-prepared-inputs-v1"
            && input.forge_commit == "3ead7e68d54bf95c6f778fd6aa7c604c036e0736"
            && input.definitions.len() == 4
            && input.cases.len() == 8,
        "prepared closed input header mismatch",
    )?;
    for row in &source.definitions {
        let definition = input
            .definitions
            .get(&row.name)
            .ok_or("prepared full source definition missing")?;
        require(
            definition.name == row.name
                && definition.oracle_id == row.oracle_id
                && definition.snapshot_printing_id == row.scryfall_id
                && definition.source_row.sha256 == row.raw_line_sha256_including_lf
                && definition.edition.to_lowercase() == row.set
                && definition.collector_number == row.collector,
            "prepared source/printing/raw-row identity mismatch",
        )?;
        let bytes = std::fs::read(&definition.source_row.path).map_err(PreparedError::Io)?;
        require(
            bytes.len() as u64 == definition.source_row.bytes
                && digest(&bytes) == definition.source_row.sha256,
            "prepared source-row file pin mismatch",
        )?;
        // Reference script/edition coordinates are retained input provenance only.
        // Native execution never reads those scripts or any reference output.
        require(
            !definition.script.path.is_empty()
                && definition.script.bytes > 0
                && definition.script.sha256.len() == 64
                && !definition.edition_file.path.is_empty()
                && definition.edition_file.bytes > 0
                && definition.edition_file.sha256.len() == 64,
            "prepared authored provenance missing",
        )?;
    }
    for (ordinal, case) in input.cases.iter().enumerate() {
        require(
            case.case_id == format!("LT{:02}", ordinal + 1),
            "prepared exact eight case order mismatch",
        )?;
    }
    Ok(())
}
struct Prepared {
    context: GameContext,
    seed: GameSeed,
    start: TurnStart,
    aliases: BTreeMap<CardId, super::Alias>,
    definitions: Vec<super::Definition>,
}
impl Prepared {
    fn initialize(&self, game: GameId) -> Result<NativeCandidate, GameError> {
        GameState::initialize_scheduled(game, self.context.clone(), self.seed.clone(), self.start)
    }
}
fn prepare(
    source: &LibraryTopSource,
    input: &Input,
    case: &Case,
) -> Result<Prepared, PreparedError> {
    source.validate().map_err(PreparedError::Source)?;
    let spell = input
        .definitions
        .get(&case.source_definition)
        .ok_or("prepared spell definition missing")?;
    let mut rows = vec![];
    let mut copies = BTreeMap::<(SeatId, String), u32>::new();
    for (alias, owner, oracle) in [
        ("A.LAND".to_string(), A, ISLAND.to_string()),
        ("A.SPELL".to_string(), A, spell.oracle_id.clone()),
    ]
    .into_iter()
    .chain(
        case.a_library_top_first
            .iter()
            .map(|a| (a.clone(), A, ISLAND.to_string())),
    )
    .chain(
        case.b_library_top_first
            .iter()
            .map(|a| (a.clone(), B, ISLAND.to_string())),
    ) {
        let count = copies.entry((owner, oracle.clone())).or_default();
        *count += 1;
        rows.push(PhysicalRow {
            alias,
            card: CardId(
                u64::try_from(rows.len()).map_err(|_| "prepared card count overflow")? + 1,
            ),
            owner,
            oracle_id: oracle,
            copy: *count,
        });
    }
    let materialized = catalogue::materialize_rows(
        &source.inventory,
        &source.registry,
        &source.definitions,
        &rows,
        Shape::LibraryTopPrepared,
    )?;
    let hand = vec!["A.LAND".into(), "A.SPELL".into()];
    let zones = [
        PreparedZone {
            owner: A,
            hand: &hand,
            library_top_first: &case.a_library_top_first,
            graveyard_top_first: &[],
            exile: &[],
        },
        PreparedZone {
            owner: B,
            hand: &[],
            library_top_first: &case.b_library_top_first,
            graveyard_top_first: &[],
            exile: &[],
        },
    ];
    let lowered = catalogue::prepare_materialized(
        materialized,
        PreparedSetup {
            start: TurnStart {
                number: 1,
                step: Step::PrecombatMain,
            },
            active: A,
            life: vec![
                spellwake_engine::setup::StartingLife {
                    player: A,
                    total: 20.into(),
                },
                spellwake_engine::setup::StartingLife {
                    player: B,
                    total: 20.into(),
                },
            ],
            zones: &zones,
            battlefield_oldest_first: &[],
            mana: vec![],
        },
    )?;
    let names = Arc::new(NameRules::literal_names());
    let context = GameContext::new(
        LIBRARY_TOP_PROFILE.into(),
        source.vocabulary.rules_revision().into(),
        format!("oracle-sha256:{TEACHING_SOURCE_SHA256}"),
        "library-top-registry11".into(),
        "library-top-prepared8-v1".into(),
        source.vocabulary.id(),
        names.id(),
    )?;
    let mut seed = GameSeed::with_uniform_life(
        vec![A, B],
        lowered.active,
        lowered.materialized.cards,
        lowered.initial_battlefield_order,
        lowered.mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        Arc::clone(&source.vocabulary),
        names,
    );
    seed.life = lowered.life;
    Ok(Prepared {
        context,
        seed,
        start: lowered.start,
        aliases: lowered.materialized.aliases,
        definitions: lowered.materialized.definitions,
    })
}
struct Run<'a> {
    state: GameState,
    prepared: &'a Prepared,
    calls: Vec<(GameInput, Result<NativeCandidate, GameError>)>,
    diagnostic: Vec<Value>,
}
impl Run<'_> {
    fn object(&self, alias: &str) -> Result<ObjectId, PreparedError> {
        let card = self
            .prepared
            .aliases
            .values()
            .find(|r| r.alias == alias)
            .ok_or("prepared physical alias absent")?
            .card;
        self.state
            .execution()
            .window()
            .cards()
            .current_identity(card)
            .ok_or_else(|| "prepared actual physical identity absent".into())
    }
    fn aliases(&self, objects: &[ObjectId]) -> Result<Vec<String>, PreparedError> {
        objects
            .iter()
            .map(|id| {
                let card = self
                    .state
                    .execution()
                    .window()
                    .cards()
                    .get(*id)
                    .and_then(|c| c.card_id())
                    .ok_or("prepared offered physical card absent")?;
                self.prepared
                    .aliases
                    .get(&card)
                    .map(|a| a.alias.clone())
                    .ok_or_else(|| "prepared offered alias absent".into())
            })
            .collect()
    }
    fn objects(&self, aliases: &[String]) -> Result<Vec<ObjectId>, PreparedError> {
        aliases.iter().map(|a| self.object(a)).collect()
    }
    fn call(
        &mut self,
        actor: SeatId,
        command: GameCommand,
        association: Value,
    ) -> Result<(), PreparedError> {
        let input = GameInput {
            basis: self.state.basis().clone(),
            actor,
            command,
        };
        let before = self.state.clone();
        let result = self.state.advance(input.clone());
        require(self.state == before, "prepared predecessor mutation")?;
        let observation = match &result {
            Ok(candidate) => diagnostic::candidate(candidate, &self.state, &self.prepared.aliases)?,
            Err(error) => projection::game_error(error),
        };
        self.diagnostic.push(json!({"ordinal":self.calls.len(),"before":diagnostic::state(&before, &self.prepared.aliases)?,"input":projection::input(&input),"authored_choice_association":association,"result":observation}));
        self.calls.push((input, result.clone()));
        self.state = result?.into_state();
        Ok(())
    }
    fn reply(&mut self, choice: DecisionReply, association: Value) -> Result<(), PreparedError> {
        let decision = self
            .state
            .pending_decision()
            .ok_or("prepared actual decision missing")?;
        self.call(
            decision.chooser(),
            GameCommand::Reply {
                decision: decision.id(),
                choice,
            },
            association,
        )
    }
}
fn run_case(
    source: &LibraryTopSource,
    input: &Input,
    case: &Case,
    game: GameId,
) -> Result<(Value, Vec<super::Definition>), PreparedError> {
    let prepared = prepare(source, input, case)?;
    let initialized = prepared.initialize(game)?;
    require(
        prepared.initialize(game) == Ok(initialized.clone()),
        "prepared initializer full Result replay mismatch",
    )?;
    let mut run = Run {
        state: initialized.state().clone(),
        prepared: &prepared,
        calls: vec![],
        diagnostic: vec![],
    };
    let initial = diagnostic::candidate(&initialized, initialized.state(), &prepared.aliases)?;
    run.call(
        A,
        GameCommand::PlayLand {
            land: run.object("A.LAND")?,
        },
        Value::Null,
    )?;
    run.call(
        A,
        GameCommand::ActivateMana {
            source: run.object("A.LAND")?,
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
        Value::Null,
    )?;
    let payment = ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: ManaType::Blue,
            amount: 1.into(),
        }],
    };
    let spell = run.object("A.SPELL")?;
    let command = if input
        .definitions
        .get(&case.source_definition)
        .ok_or("prepared spell definition absent")?
        .oracle_id
        == SCOUR
    {
        GameCommand::CastPaid {
            spell,
            target: TargetRef::Player(seat(
                case.target_player
                    .as_deref()
                    .ok_or("prepared target missing")?,
            )?),
            payment,
        }
    } else {
        GameCommand::CastUntargeted {
            spell,
            payment: Some(payment),
        }
    };
    run.call(A, command, Value::Null)?;
    for _ in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            run.state.execution().window().phase()
        else {
            return Err("prepared cast prefix priority missing".into());
        };
        run.call(holder, GameCommand::Pass, Value::Null)?;
    }
    let mut partitions = 0;
    let mut orders = 0;
    if let Some(authored) = &case.partition {
        let decision = run
            .state
            .pending_decision()
            .ok_or("prepared authored partition frame missing")?;
        let RuleFrame::LibraryTop(frame) = decision.frame() else {
            return Err("prepared wrong partition frame kind".into());
        };
        require(
            decision.chooser() == seat(&authored.actor)?
                && run.aliases(frame.domain_top_first())? == authored.offered_aliases,
            "prepared actual partition actor/domain mismatch",
        )?;
        let top = run.objects(&authored.top_first)?;
        let rest = run.objects(&authored.other_top_first)?;
        let kind = frame.kind();
        require(
            matches!(
                (case.source_definition.as_str(), kind),
                ("Opt", LibraryChoiceKind::Scry) | ("Consider", LibraryChoiceKind::Surveil)
            ),
            "prepared operation kind mismatch",
        )?;
        let reply = DecisionReply::LibraryTop(match kind {
            LibraryChoiceKind::Search => {
                return Err("unexpected search in prepared scry/surveil".into());
            }
            LibraryChoiceKind::Scry => LibraryTopReply::Scry {
                top_first: top,
                bottom_top_first: rest,
            },
            LibraryChoiceKind::Surveil => LibraryTopReply::Surveil {
                top_first: top,
                to_graveyard: rest,
            },
        });
        run.reply(reply, json!({"authored_partition":authored,"orientation":"scry top/bottom top-first; surveil selected singleton"}))?;
        partitions += 1;
    }
    if !case.graveyard_movement_order.is_empty() {
        let decision = run
            .state
            .pending_decision()
            .ok_or("prepared authored owner-order frame missing")?;
        let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
            return Err("prepared wrong owner-order frame kind".into());
        };
        let target = seat(
            case.target_player
                .as_deref()
                .ok_or("prepared mill target missing")?,
        )?;
        let offered = if target == A {
            &case.a_library_top_first
        } else {
            &case.b_library_top_first
        };
        require(
            decision.chooser() == target
                && run.aliases(frame.domain())?
                    == offered
                        .get(..2)
                        .ok_or("prepared owner-order domain too short")?,
            "prepared actual owner-order actor/domain mismatch",
        )?;
        // Authored vectors are physical application order. Native selection_batch
        // accepts top-to-bottom and reverses it for physical application. Translate
        // before calling the native API; never repair observations after execution.
        let top_first = case
            .graveyard_movement_order
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        let reply = DecisionReply::OrderResolutionGraveyard(run.objects(&top_first)?);
        run.reply(reply, json!({"authored_movement_order":case.graveyard_movement_order,"native_top_first":top_first,"translation":"reverse physical application order before native reply"}))?;
        orders += 1;
    }
    require(
        run.state.pending_decision().is_none()
            && run.state.pending_turn_work().is_none()
            && run.state.execution().window().entries().is_empty()
            && run.state.execution().window().resolving().is_none()
            && run.state.execution().window().active_player() == A
            && matches!(run.state.execution().window().phase(), WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == A)
            && run.state.turn().is_some_and(|t| {
                t.position().number == 1 && t.position().step == Step::PrecombatMain
            })
            && run.state.outcome().is_none(),
        "prepared unique settled priority cut not reached",
    )?;
    require(
        run.state
            .execution()
            .window()
            .cards()
            .get(run.object("A.SPELL")?)
            .is_some_and(|c| c.zone() == ObjectZone::Graveyard(A)),
        "prepared settled spell is not in caster graveyard",
    )?;
    let draws = run
        .calls
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok())
        .flat_map(|c| &c.draw_journal().events)
        .collect::<Vec<_>>();
    require(
        draws.len() == 1
            && matches!(draws[0], spellwake_engine::draw::DrawEvent::Drawn { player, .. } if *player == A),
        "prepared caster draw continuation mismatch",
    )?;
    let payments = run
        .calls
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok())
        .filter_map(|c| c.mana_payment())
        .collect::<Vec<_>>();
    require(
        payments.len() == 1
            && payments[0].payer == A
            && payments[0].cost == ManaCost::one(ManaType::Blue)
            && payments[0].spend
                == vec![ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: 1.into(),
                }],
        "prepared actual paid prefix mismatch",
    )?;
    let mut replay = prepared.initialize(game)?.into_state();
    for (input, expected) in &run.calls {
        let before = replay.clone();
        let actual = replay.advance(input.clone());
        require(
            actual == *expected && replay == before,
            "prepared full original input/Result replay mismatch",
        )?;
        if let Ok(next) = actual {
            replay = next.into_state();
        }
    }
    require(
        replay == run.state,
        "prepared full final state replay mismatch",
    )?;
    Ok((
        json!({"case_id":case.case_id,"source_definition":case.source_definition,"input_premise":case,"initialization":{"context":projection::context(&prepared.context),"seed":projection::game_seed_input(&prepared.seed),"start":{"turn":1,"step":"PrecombatMain"},"result":initial},"calls":run.diagnostic,"settled_priority":diagnostic::state(&run.state,&prepared.aliases)?,"consumed_partition_frames":partitions,"consumed_owner_order_frames":orders,"full_native_initialization_result_replay":true,"full_native_each_input_result_replay":true,"full_native_final_state_replay":true}),
        prepared.definitions,
    ))
}
/// Execute exactly the frozen eight authored preparations through native root APIs.
/// The returned diagnostic is privileged and bounded, not a complete state codec.
///
/// # Errors
/// Rejects foreign input bytes/source pins, incomplete source admission, physical
/// preparation, native transition/choice mismatches, unsupported observations or replay gaps.
pub fn execute(source: &LibraryTopSource, bytes: &[u8]) -> Result<Value, PreparedError> {
    let input = parse(bytes)?;
    source_admission(source, &input)?;
    let mut cases = vec![];
    let mut definitions = BTreeMap::new();
    for (ordinal, case) in input.cases.iter().enumerate() {
        let (result, used) = run_case(source, &input, case, GameId(88600 + ordinal as u128))?;
        for definition in used {
            let value = program::view(&definition.program);
            require(
                program::decode(value.clone(), &source.vocabulary)? == definition.program,
                "prepared complete Program codec roundtrip mismatch",
            )?;
            if let Some(previous) = definitions.insert(definition.oracle_id, value.clone()) {
                require(previous == value, "prepared definition Program instability")?;
            }
        }
        cases.push(result);
    }
    require(
        definitions.len() == 4,
        "prepared complete four Program bindings missing",
    )?;
    Ok(
        json!({"schema":"library-top-native-prepared-diagnostic-v2","scope":"privileged exact prepared8; bounded actual observer; in-process full native replay only; no public/archive/reference agreement", "authored_input_sha256":INPUT_SHA256,"authored_input_bytes":bytes.len(),"authored_input":packet::unique_json(bytes)?,"source_descriptor":packet::unique_json(super::DEFINITIONS)?,"registry_coverage":source.registry.coverage(),"definition_programs":definitions,"definition_program_origin":"maintained Registry bindings lowered by the shared catalogue across the eight cases; distinct from per-case physical seeds","cases":cases}),
    )
}

#[cfg(test)]
mod tests;
