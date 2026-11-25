//! Local AI benchmark host over the maintained application player adapter.
//! This is in-process candidate acceptance, not production admission or SQL/Auth.
use serde::Deserialize;
use serde_json::{Value, json};
use spellwake_application::{
    amount::Amount,
    native_game::*,
    player_wire::{
        PlayerAbilityKind, PlayerAction, PlayerBasis, PlayerCardType, PlayerCharacteristics,
        PlayerCommand, PlayerSnapshot, decode_player_record,
    },
};
use spellwake_card_programs::OracleId;
use spellwake_engine::{
    characteristics::ObjectCharacteristics,
    draw::GameOutcome,
    effects::DamageAllocationDomain,
    game::{GameContext, GameState},
    object_identity::GameObject,
    occurrence::{ExecutionBindings, WaitingInstance},
    priority::{PriorityPhase, SeatId},
    program::{ExecutableBody, Program},
    stack_window::{StackEntry, StackKind, WindowPhase},
    token::TokenEntryPreview,
};
use std::{
    error::Error,
    fs,
    io::{self, BufRead, Write},
    path::PathBuf,
    time::Instant,
};
use uuid::Uuid;
type Failure = Box<dyn Error>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    metadata: PathBuf,
    snapshot: PathBuf,
    vocabulary: PathBuf,
    run: PathBuf,
    seed: u64,
    game_id: Uuid,
    decks: [Vec<String>; 2],
}
fn seat(index: usize) -> Uuid {
    Uuid::from_u128(101 + index as u128)
}
fn roster() -> Vec<NativeSeatBinding> {
    (0..2)
        .map(|i| NativeSeatBinding {
            seat_id: seat(i),
            native_seat: SeatId(i as u32),
        })
        .collect()
}
fn binding(state: &GameState, viewer: Uuid) -> Result<NativeBinding, NativeBridgeError> {
    NativeBinding::new(
        Uuid::from_u128(103),
        state.basis().clone(),
        roster(),
        viewer,
    )
}
fn entropy(seed: u64) -> Vec<spellwake_application::player_wire::NativeRevision> {
    let mut value = seed;
    (0..4096)
        .map(|_| {
            value = value.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = value;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            (z ^ (z >> 31)).into()
        })
        .collect()
}
fn opening(
    config: &Config,
    source: &NativeProgramSource,
) -> Result<PreparedNativeOpening, Failure> {
    if config.decks.iter().any(|deck| deck.len() != 60) {
        return Err("benchmark requires two 60-card lists".into());
    }
    let mut participants = Vec::new();
    for (i, deck) in config.decks.iter().enumerate() {
        let mut cards = Vec::new();
        for id in deck {
            let oracle = OracleId::try_from(id.clone())?;
            if !source.supports(&oracle) {
                return Err("unsupported benchmark card".into());
            }
            cards.push(NativeInitialCard {
                oracle_id: oracle,
                zone: NativeInitialZone::Library,
            });
        }
        participants.push(NativeInitialParticipant {
            seat_id: seat(i),
            life: 20.into(),
            cards,
        });
    }
    Ok(prepare_initial(
        NativeInitialIds {
            game_id: config.game_id,
            context_id: Uuid::from_u128(103),
            release_id: Uuid::from_u128(104),
            profile_id: Uuid::from_u128(105),
        },
        source,
        NativeInitialSpec {
            first_player_seat_id: seat(0),
            participants,
            entropy: entropy(config.seed),
        },
    )?)
}
/// Source-only descriptions join actual native definitions to frozen printed data.
/// Token cards are excluded from this first benchmark; no guessed descriptions.
struct Display<'a> {
    source: &'a NativeProgramSource,
    context: GameContext,
}
impl Display<'_> {
    fn description(
        &self,
        program: &Program,
    ) -> Result<PlayerCharacteristics, NativeProjectionError> {
        self.source
            .printed_card(program)
            .map_err(NativeProjectionError::metadata)?
            .map(|card| card.characteristics().clone())
            .ok_or(NativeProjectionError::MissingDisplay)
    }
}
impl NativeDisplaySource for Display<'_> {
    fn context(&self) -> &GameContext {
        &self.context
    }
    fn scope(&self) -> NativeDisplayScope {
        NativeDisplayScope::SourceOnly
    }
    fn object(
        &self,
        _: &GameObject,
        definition: &Program,
    ) -> Result<NativeObjectDisplay, NativeProjectionError> {
        Ok(NativeObjectDisplay {
            definition_id: None,
            printing_id: None,
            custom_card_revision_id: None,
            characteristics: self.description(definition)?,
        })
    }
    fn ability(
        &self,
        _: &GameObject,
        _: &ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError> {
        Ok(match ability {
            NativeAbilityReference::Mana(_) => "Mana ability",
            NativeAbilityReference::Activated(_) => "Activated ability",
            NativeAbilityReference::Triggered(_) => "Triggered ability",
            NativeAbilityReference::Continuous(_) => "Static ability",
        }
        .into())
    }
    fn stack(
        &self,
        entry: &StackEntry,
        _: Option<&ExecutableBody>,
        _: Option<&ExecutionBindings>,
    ) -> Result<String, NativeProjectionError> {
        Ok(match entry.kind() {
            StackKind::CardSpell(_) => "Card spell",
            StackKind::SpellCopy { .. } => "Spell copy",
            StackKind::ActivatedAbility { .. } => "Activated ability",
            StackKind::TriggeredAbility { .. } => "Triggered ability",
            StackKind::Speed => "Speed trigger",
            StackKind::Monarch => "Monarch trigger",
            StackKind::DelayedCopy { .. } => "Delayed triggered ability",
            StackKind::Emblem { .. } => "Emblem ability",
        }
        .into())
    }
    fn token_preview(&self, _: &TokenEntryPreview) -> Result<String, NativeProjectionError> {
        Err(NativeProjectionError::MissingDisplay)
    }
    fn trigger(&self, _: &WaitingInstance) -> Result<String, NativeProjectionError> {
        Ok("Triggered ability".into())
    }
    fn prevention_attempt(
        &self,
        attempt: &DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError> {
        Ok(format!("{} damage", String::from(attempt.amount().clone())))
    }
}
fn actor(state: &GameState) -> Result<usize, Failure> {
    let player = if let Some(decision) = state.pending_decision() {
        decision.chooser()
    } else if let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
        state.execution().window().phase()
    {
        holder
    } else {
        return Err("unsupported native boundary: no player decision or ready priority".into());
    };
    match player {
        SeatId(0) => Ok(0),
        SeatId(1) => Ok(1),
        _ => Err("foreign actor".into()),
    }
}
fn outcome(state: &GameState) -> Value {
    match state.outcome() {
        None => Value::Null,
        Some(GameOutcome::Winner(winner)) => json!({"kind":"winner","seat":winner.0}),
        Some(GameOutcome::Draw) => json!({"kind":"draw"}),
    }
}
fn save_new(path: &std::path::Path, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn emit(value: &Value) -> Result<(), Failure> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
fn replay(
    config: &Config,
    source: &NativeProgramSource,
    state: &GameState,
    journal: &[(Uuid, Vec<u8>)],
) -> Result<(), Failure> {
    let original = opening(config, source)?;
    if original.original_bytes() != fs::read(config.run.join("initial.json"))? {
        return Err("original root bytes changed".into());
    }
    let mut replayed = original.into_parts().2.into_state();
    for (viewer, bytes) in journal {
        replayed = restore_input(&replayed, binding(&replayed, *viewer)?, bytes)?
            .into_parts()
            .3
            .into_state();
    }
    if &replayed != state {
        return Err("full native replay mismatch".into());
    }
    Ok(())
}
/// Basic public affordances use actual native validation, never Python rules.
fn basic_actions(
    scope: &NativeScope<'_>,
    snapshot: &PlayerSnapshot,
) -> Result<Vec<PlayerAction>, Failure> {
    let mut available = Vec::new();
    if !snapshot.decisions.is_empty() {
        return Ok(available);
    }
    for object in &snapshot.visible_objects {
        if object.owner_seat_id != snapshot.viewer_seat_id
            && object.controller_seat_id != Some(snapshot.viewer_seat_id)
        {
            continue;
        }
        let mut candidates = Vec::new();
        if object
            .public_properties
            .as_ref()
            .is_some_and(|p| p.card_types.contains(&PlayerCardType::Land))
        {
            candidates.push(PlayerAction::PlayLand {
                source: object.handle.clone(),
            });
        }
        for ability in &object.abilities {
            if ability.kind == PlayerAbilityKind::Mana {
                candidates.push(PlayerAction::ActivateMana {
                    source: object.handle.clone(),
                    ability: ability.handle.clone(),
                });
            }
        }
        for action in candidates {
            let command = PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: scope.basis().clone(),
                action: action.clone(),
            };
            if scope.propose(&command).is_ok() {
                available.push(action);
            }
        }
    }
    Ok(available)
}
fn rejection(error: &NativeBridgeError) -> &'static str {
    use spellwake_engine::{
        effects::EffectError, game::GameError, stack_window::StackWindowError, turn::TurnError,
    };
    match error {
        NativeBridgeError::InvalidReference => {
            "reference unavailable: use only this snapshot's handles"
        }
        NativeBridgeError::Native(e) => match e.as_ref() {
            GameError::Turn(TurnError::LandLimitReached) => {
                "land play allowance already used this turn"
            }
            GameError::Effect(EffectError::Window(StackWindowError::SourceTapped)) => {
                "source is already tapped"
            }
            GameError::Effect(EffectError::InsufficientMana) => {
                "insufficient mana in the current pool"
            }
            GameError::Effect(EffectError::ManaPaymentRequired) => {
                "explicit mana payment is required"
            }
            GameError::Effect(EffectError::InvalidManaPayment(_)) => {
                "invalid allocation for the actual cost or pool"
            }
            GameError::Effect(EffectError::Timing(_)) => {
                "action is outside its allowed timing window"
            }
            GameError::Effect(EffectError::NotOwnHand) => "source is not in your current hand",
            GameError::Effect(EffectError::IllegalTarget) => "target is not legal for this action",
            GameError::DecisionRequired => {
                "answer the required decision before taking a priority action"
            }
            _ => {
                "native transition rejected: reconsider current source, timing, targets and payment"
            }
        },
        _ => "invalid current player intent or reference",
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BatchWire {
    request_id: Uuid,
    basis: PlayerBasis,
    action: BatchAction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchAction {
    kind: String,
    mana: Vec<PlayerAction>,
    cast: PlayerAction,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CommitGroup {
    actor: usize,
    base_revision: u64,
    final_revision: u64,
    first_input: usize,
    records: Vec<String>,
}
fn commit_group(path: &std::path::Path, group: &CommitGroup) -> Result<(), Failure> {
    let temporary = path.with_extension("pending");
    save_new(&temporary, &serde_json::to_vec(group)?)?;
    if path.exists() {
        return Err("operation already committed".into());
    }
    fs::rename(&temporary, path)?;
    fs::File::open(path.parent().ok_or("missing group parent")?)?.sync_all()?;
    Ok(())
}
fn batch_commands(bytes: &[u8]) -> Result<Vec<PlayerCommand>, Failure> {
    let wire: BatchWire = spellwake_application::decode_wire_object(bytes)?;
    if wire.request_id.is_nil() || wire.action.kind != "cast-batch" || wire.action.mana.len() > 8 {
        return Err("invalid batch shape".into());
    }
    let mut actions = wire.action.mana;
    actions.push(wire.action.cast);
    Ok(actions
        .into_iter()
        .enumerate()
        .map(|(i, action)| PlayerCommand {
            request_id: if i == 0 {
                wire.request_id
            } else {
                Uuid::new_v4()
            },
            basis: wire.basis.clone(),
            action,
        })
        .collect())
}
fn run() -> Result<(), Failure> {
    let path = std::env::args_os().nth(1).ok_or("config path required")?;
    let config: Config = spellwake_application::decode_wire_object(&fs::read(path)?)?;
    if config.game_id.is_nil() {
        return Err("nil game identity".into());
    }
    let _host_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.run.join("native-host.lock"))?;
    _host_lock.try_lock()?;
    let resume = std::env::args().nth(2).as_deref() == Some("--resume");
    if !resume {
        fs::create_dir(config.run.join("journal"))?;
        fs::create_dir(config.run.join("groups"))?;
    }
    let started = Instant::now();
    let source = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &fs::read(&config.metadata)?,
        fs::read(&config.snapshot)?,
        &fs::read(&config.vocabulary)?,
    )?;
    let initial = opening(&config, &source)?;
    if resume {
        if fs::read(config.run.join("initial.json"))? != initial.original_bytes() {
            return Err("resume original root mismatch".into());
        }
    } else {
        save_new(&config.run.join("initial.json"), initial.original_bytes())?;
    }
    let mut state = initial.into_parts().2.into_state();
    let display = Display {
        source: &source,
        context: state.basis().context.clone(),
    };
    let mut journal: Vec<(Uuid, Vec<u8>)> = Vec::new();
    if resume {
        if fs::read_dir(config.run.join("groups"))?.any(|entry| {
            entry.ok().is_some_and(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "pending")
            })
        }) {
            return Err("uncommitted pending group: explicit recovery review required".into());
        }
        let mut paths = fs::read_dir(config.run.join("groups"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        });
        paths.sort();
        for (sequence, path) in paths.iter().enumerate() {
            if path.file_name().and_then(|name| name.to_str())
                != Some(&format!("{sequence:06}.json"))
            {
                return Err("noncontiguous committed groups".into());
            }
            let group: CommitGroup = spellwake_application::decode_wire_object(&fs::read(path)?)?;
            if group.actor > 1
                || group.records.is_empty()
                || group.records.len() > 9
                || group.first_input != journal.len()
                || group.base_revision != state.basis().revision
            {
                return Err("invalid committed group basis".into());
            }
            for text in group.records {
                let bytes = text.into_bytes();
                state = restore_input(&state, binding(&state, seat(group.actor))?, &bytes)?
                    .into_parts()
                    .3
                    .into_state();
                journal.push((seat(group.actor), bytes));
            }
            if state.basis().revision != group.final_revision {
                return Err("committed group final basis mismatch".into());
            }
        }
        replay(&config, &source, &state, &journal)?;
        save_new(
            &config.run.join("resume-proof.json"),
            &serde_json::to_vec_pretty(
                &json!({"status":"prefix_replayed","accepted":journal.len(),"revision":state.basis().revision.to_string(),"runtime_id":native_runtime_identity(),"all_prior_delegation_revoked":true}),
            )?,
        )?;
    }
    let recovered_prefix = journal.len();
    let mut operations = fs::read_dir(config.run.join("groups"))?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .count();
    let verify_pause = resume && std::env::args().nth(3).as_deref() == Some("--verify-and-pause");
    let mut lines = io::stdin().lock().lines();
    let mut last_accept: Option<Value> = None;
    let mut native_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut timing = json!({"initialization_ms": native_ms});
    loop {
        if verify_pause && journal.len() > recovered_prefix && state.outcome().is_none() {
            replay(&config, &source, &state, &journal)?;
            emit(
                &json!({"status":"success","data":{"paused":true,"outcome":null,"replay_verified":true,"accepted":journal.len(),"operations":operations,"last_accept":last_accept,"native_ms":native_ms,"timing":timing,"runtime_id":native_runtime_identity()}}),
            )?;
            return Ok(());
        }
        if state.outcome().is_some() {
            let replay_start = Instant::now();
            replay(&config, &source, &state, &journal)?;
            timing["terminal_replay_ms"] = json!(replay_start.elapsed().as_secs_f64() * 1000.0);
            emit(
                &json!({"status":"success","data":{"outcome":outcome(&state),"replay_verified":true,"accepted":journal.len(),"operations":operations,"last_accept":last_accept,"native_ms":native_ms,"timing":timing,"runtime_id":native_runtime_identity()}}),
            )?;
            return Ok(());
        }
        let index = actor(&state)?;
        let view_start = Instant::now();
        let mut scope = NativeScope::new(&state, binding(&state, seat(index))?)?;
        let metadata = NativeProjectionMetadata {
            through_event_sequence: Amount::from(journal.len() as u64),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let snapshot = scope.project(&metadata, &display)?;
        let projection_ms = view_start.elapsed().as_secs_f64() * 1000.0;
        let available_start = Instant::now();
        let available = basic_actions(&scope, &snapshot)?;
        let availability_ms = available_start.elapsed().as_secs_f64() * 1000.0;
        emit(
            &json!({"status":"success","data":{"actor":index,"required":state.pending_decision().is_some(),"snapshot":snapshot,"basic_actions":available,"availability_ms":availability_ms,"accepted":journal.len(),"operations":operations,"outcome":null,"last_accept":last_accept,"native_ms":native_ms,"timing":timing,"projection_ms":projection_ms,"runtime_id":native_runtime_identity(),"program_source_id":source.identity(),"fully_qualified_cards":source.fully_qualified_cards()}}),
        )?;
        // Retain this exact scope/handle registry through validation and encoding.
        loop {
            let Some(line) = lines.next() else {
                return Err("controller ended before native result".into());
            };
            let line = line?;
            if line.len() > 262_144 {
                return Err("oversize player command".into());
            }
            let input_start = Instant::now();
            let parsed: Value = match spellwake_application::decode_wire_object(line.as_bytes()) {
                Ok(value) => value,
                Err(_) => {
                    emit(&json!({"status":"fail","data":{"action":"invalid command JSON"}}))?;
                    continue;
                }
            };
            let is_batch = parsed
                .get("action")
                .and_then(|action| action.get("kind"))
                .and_then(Value::as_str)
                == Some("cast-batch");
            let transition_start = Instant::now();
            let (next, records, kind, request_id) = if is_batch {
                let commands = match batch_commands(line.as_bytes()) {
                    Ok(commands) => commands,
                    Err(_) => {
                        emit(
                            &json!({"status":"fail","data":{"action":"invalid cast-batch shape"}}),
                        )?;
                        continue;
                    }
                };
                let request_id = commands[0].request_id;
                match scope.prepare_cast_batch(&commands) {
                    Ok(batch) => {
                        let (state, records) = batch.into_parts();
                        (state, records, "cast-batch".to_owned(), request_id)
                    }
                    Err(error) => {
                        let detail = match &error {
                            NativeCastBatchError::Bridge { cause, .. } => rejection(cause),
                            _ => {
                                "batch shape, required boundary or original input reconstruction rejected"
                            }
                        };
                        emit(
                            &json!({"status":"fail","data":{"action":error.to_string(),"detail":detail,"batch_committed":false,"accepted":journal.len(),"native_rejection_ms":transition_start.elapsed().as_secs_f64()*1000.0}}),
                        )?;
                        continue;
                    }
                }
            } else {
                let command: PlayerCommand = match decode_player_record(line.as_bytes()) {
                    Ok(value) => value,
                    Err(_) => {
                        emit(&json!({"status":"fail","data":{"action":"invalid player command"}}))?;
                        continue;
                    }
                };
                let prepared = match scope.propose(&command) {
                    Ok(value) => value,
                    Err(error) => {
                        emit(
                            &json!({"status":"fail","data":{"action":rejection(&error),"accepted":journal.len(),"native_rejection_ms":transition_start.elapsed().as_secs_f64()*1000.0}}),
                        )?;
                        continue;
                    }
                };
                let record = scope.encode_input(&prepared)?;
                let restored = restore_input(&state, binding(&state, seat(index))?, &record)?;
                if restored.candidate() != prepared.candidate() {
                    return Err("input reconstruction mismatch".into());
                }
                let kind = serde_json::to_value(&command.action)?["kind"]
                    .as_str()
                    .ok_or("missing action kind")?
                    .to_owned();
                (
                    prepared.into_parts().3.into_state(),
                    vec![record],
                    kind,
                    command.request_id,
                )
            };
            let transition_ms = transition_start.elapsed().as_secs_f64() * 1000.0;
            let persistence_start = Instant::now();
            let first_input = journal.len();
            let input_count = records.len();
            let group = CommitGroup {
                actor: index,
                base_revision: state.basis().revision,
                final_revision: next.basis().revision,
                first_input,
                records: records
                    .iter()
                    .map(|record| String::from_utf8(record.clone()))
                    .collect::<Result<Vec<_>, _>>()?,
            };
            // Recovery trusts only this all-or-nothing accepted group. Journal files
            // below are derived exports and can never authorize a partial batch.
            commit_group(
                &config
                    .run
                    .join("groups")
                    .join(format!("{operations:06}.json")),
                &group,
            )?;
            for record in records {
                save_new(
                    &config
                        .run
                        .join("journal")
                        .join(format!("{:06}-{index}.json", journal.len())),
                    &record,
                )?;
                journal.push((seat(index), record));
            }
            operations += 1;
            let persistence_ms = persistence_start.elapsed().as_secs_f64() * 1000.0;
            state = next;
            let baseline_start = Instant::now();
            let continuation_ready = state.pending_decision().is_none()
                && state.outcome().is_none()
                && (kind == "pass"
                    || matches!(state.execution().window().phase(),WindowPhase::Priority(PriorityPhase::Ready{holder}) if holder==SeatId(index as u32)));
            let baseline = if state.outcome().is_none() {
                let mut post = NativeScope::new(&state, binding(&state, seat(index))?)?;
                Some(post.project(
                    &NativeProjectionMetadata {
                        through_event_sequence: Amount::from(journal.len() as u64),
                        deadline_at: None,
                        disclosure: NativeDisclosure::LiveParticipant,
                    },
                    &display,
                )?)
            } else {
                None
            };
            last_accept = Some(
                json!({"actor":index,"kind":kind,"request_id":request_id,"revision":state.basis().revision.to_string(),"actor_snapshot":baseline,"continuation_ready":continuation_ready,"input_count":input_count,"first_input":first_input,"operation":operations-1}),
            );
            native_ms = input_start.elapsed().as_secs_f64() * 1000.0;
            timing = json!({"staging_transition_and_codec_ms":transition_ms,"codec_reconstruction_included_in_staging":true,"persistence_ms":persistence_ms,"authority_projection_ms":baseline_start.elapsed().as_secs_f64()*1000.0,"total_host_ms":native_ms});
            break;
        }
    }
}
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--identity") {
        if let Err(error) =
            emit(&json!({"status":"success","data":{"runtime_id":native_runtime_identity()}}))
        {
            eprintln!("native identity output failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        eprintln!("native benchmark stopped: {error}");
        let code = if error.downcast_ref::<std::io::Error>().is_some() {
            "native_io_error"
        } else if error.downcast_ref::<NativeProjectionError>().is_some() {
            "native_projection_failure"
        } else if error.downcast_ref::<NativeInitialError>().is_some() {
            "native_initialization_failure"
        } else if error.downcast_ref::<NativeCodecError>().is_some() {
            "native_replay_codec_failure"
        } else {
            "native_host_failure"
        };
        let _ = emit(&json!({"status":"error","message":error.to_string(),"code":code}));
        std::process::exit(1);
    }
}
