//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! Actual native fixed-tap mana activations; reference names identify authored
//! outputs, never catalogue bindings. No expected/reference files are consumed.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::combat::CombatRules;
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    effects::{EffectError, Program, SeedCard},
    game::{
        CandidateBase, GameBasis, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
    },
    mana::{FloatingMana, ManaAbility, ManaAbilityId, ManaOutput, ManaProductionRecord, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId},
    priority::SeatId,
};
use std::{collections::BTreeMap, process::ExitCode};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
type Pool = BTreeMap<&'static str, RulesInteger>;
type AuthoredAbility = (&'static str, u64, Vec<(ManaType, i64)>);
type AbilityInput<'a> = (&'static str, u64, &'a [(ManaType, i64)]);

#[derive(Clone, Serialize)]
struct SourceView {
    card: &'static str,
    zone: String,
    owner: &'static str,
    controller: Option<&'static str>,
    tapped: Option<bool>,
}
#[derive(Clone, Serialize)]
struct Snapshot {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

    label: &'static str,
    sources: BTreeMap<&'static str, SourceView>,
    pools: BTreeMap<&'static str, Pool>,
    stack_size: usize,
    accepted: Option<bool>,
}
#[derive(Clone, Serialize)]
struct BasisView {
    game: String,
    context: serde_json::Value,
    epoch: u64,
    revision: u64,
}
impl From<&GameBasis> for BasisView {
    fn from(basis: &GameBasis) -> Self {
        Self {
            game: format!("{:?}", basis.game),
            context: spellwake_reference_tools::projection::context(&basis.context),
            epoch: basis.epoch,
            revision: basis.revision,
        }
    }
}
#[derive(Serialize)]
struct NativeSource {
    object: String,
    physical_card: u64,
}
#[derive(Serialize)]
struct NativeState {
    basis: BasisView,
    sources: BTreeMap<&'static str, NativeSource>,
    snapshot: Snapshot,
    stack_entries: Vec<String>,
    spell_copies: String,
    priority: String,
}
#[derive(Serialize)]
struct ProductionView {
    source: String,
    ability: serde_json::Value,
    controller: &'static str,
    produced: Pool,
}
impl From<&ManaProductionRecord> for ProductionView {
    fn from(receipt: &ManaProductionRecord) -> Self {
        Self {
            source: format!("{:?}", receipt.source),
            ability: spellwake_reference_tools::mana_ref::view(receipt.ability),
            controller: seat(receipt.controller),
            produced: amounts(&receipt.produced),
        }
    }
}
#[derive(Serialize)]
struct Attempt {
    label: &'static str,
    actor: &'static str,
    source_alias: &'static str,
    requested_source: String,
    requested_ability: serde_json::Value,
    before: NativeState,
    after: NativeState,
    candidate_base: Option<BasisView>,
    production: Option<ProductionView>,
    payment_receipt_present: bool,
    input_unchanged: bool,
    error: Option<String>,
}
#[derive(Serialize)]
struct Followup {
    command: &'static str,
    actor: &'static str,
    before: NativeState,
    after: NativeState,
    candidate_base: BasisView,
    production_receipt_present: bool,
    payment_receipt_present: bool,
    input_unchanged: bool,
}
#[derive(Serialize)]
struct NativeProbe {
    id: &'static str,
    initialization_production_present: bool,
    initialization_payment_present: bool,
    initial: NativeState,
    attempts: Vec<Attempt>,
    followup: Followup,
}
#[derive(Serialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}
#[derive(Serialize)]
struct CommonProbe {
    id: &'static str,
    source_cards: BTreeMap<&'static str, &'static str>,
    selected_ability: OneOrMany<&'static str>,
    produced: OneOrMany<Pool>,
    snapshots: Vec<Snapshot>,
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    kind: &'static str,
    native_driver: &'static str,
    program_source: &'static str,
    scope: &'static str,
    numeric_encoding: &'static str,
    catalogue_bindings: u32,
    fully_qualified_cards: u32,
    full_game_comparisons: u32,
    scenarios: BTreeMap<&'static str, Vec<CommonProbe>>,
    native_observations: Vec<NativeProbe>,
}

struct Source {
    alias: &'static str,
    name: &'static str,
    card: CardId,
    owner: SeatId,
    abilities: Vec<AuthoredAbility>,
}
struct Activation {
    label: &'static str,
    source: &'static str,
    ability: &'static str,
}
struct Probe {
    group: &'static str,
    id: &'static str,
    sources: Vec<Source>,
    activations: Vec<Activation>,
}

fn seat(player: SeatId) -> &'static str {
    match player {
        A => "A",
        B => "B",
        _ => "unknown",
    }
}
fn symbol(mana_type: ManaType) -> &'static str {
    match mana_type {
        ManaType::White => "W",
        ManaType::Blue => "U",
        ManaType::Black => "B",
        ManaType::Red => "R",
        ManaType::Green => "G",
        ManaType::Colourless => "C",
    }
}
fn amounts(values: &BTreeMap<ManaType, RulesInteger>) -> Pool {
    ManaType::ALL
        .into_iter()
        .map(|kind| {
            (
                symbol(kind),
                values.get(&kind).cloned().unwrap_or_else(|| 0.into()),
            )
        })
        .collect()
}
fn identity(state: &GameState, card: CardId) -> Result<ObjectId, GameError> {
    state
        .execution()
        .window()
        .cards()
        .current_identity(card)
        .ok_or(EffectError::UnknownObject.into())
}
fn snapshot(
    state: &GameState,
    sources: &[Source],
    label: &'static str,
    accepted: Option<bool>,
) -> Result<Snapshot, GameError> {
    let sources = sources
        .iter()
        .map(|source| {
            let id = identity(state, source.card)?;
            let object = state
                .execution()
                .window()
                .cards()
                .get(id)
                .ok_or(EffectError::UnknownObject)?;
            Ok((
                source.alias,
                SourceView {
                    card: source.name,
                    zone: format!("{:?}", object.zone()),
                    owner: seat(object.owner()),
                    controller: object.controller().map(seat),
                    tapped: object.tapped(),
                },
            ))
        })
        .collect::<Result<_, GameError>>()?;
    Ok(Snapshot {
        capture_point: spellwake_reference_tools::capture_points::token(
            concat!(file!(), "::snapshot"),
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("current native observation point"),
        label,
        sources,
        pools: [A, B]
            .into_iter()
            .map(|player| {
                (
                    seat(player),
                    ManaType::ALL
                        .into_iter()
                        .map(|kind| (symbol(kind), state.execution().mana(player, kind)))
                        .collect(),
                )
            })
            .collect(),
        stack_size: state.execution().window().entries().len(),
        accepted,
    })
}
fn native_state(
    state: &GameState,
    sources: &[Source],
    label: &'static str,
    accepted: Option<bool>,
) -> Result<NativeState, GameError> {
    Ok(NativeState {
        basis: state.basis().into(),
        sources: sources
            .iter()
            .map(|source| {
                let id = identity(state, source.card)?;
                let object = state
                    .execution()
                    .window()
                    .cards()
                    .get(id)
                    .ok_or(EffectError::UnknownObject)?;
                Ok((
                    source.alias,
                    NativeSource {
                        object: format!("{:?}", object.id()),
                        physical_card: object.card_id().expect("source-authored card-only trace").0,
                    },
                ))
            })
            .collect::<Result<_, GameError>>()?,
        snapshot: snapshot(state, sources, label, accepted)?,
        stack_entries: state
            .execution()
            .window()
            .entries()
            .iter()
            .map(|entry| format!("{entry:?}"))
            .collect(),
        spell_copies: format!("{:?}", state.execution().window().spell_copies()),
        priority: format!("{:?}", state.execution().window().phase()),
    })
}
fn run_probe(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    index: u128,
    probe: Probe,
) -> Result<(CommonProbe, NativeProbe), GameError> {
    let initialized = spellwake_reference_tools::current_capture::initialize(
        GameId(index),
        GameContext::new(
            "native-mana-abilities-v1".into(),
            vocabulary.rules_revision().to_owned(),
            "synthetic-no-catalogue-binding".into(),
            "authored-fixed-tap-land-v1".into(),
            probe.id.into(),
            vocabulary.id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )?,
        {
            let cards: Vec<SeedCard> = probe
                .sources
                .iter()
                .map(|source| SeedCard {
                    card: source.card,
                    owner: source.owner,
                    destination: ObjectDestination::Battlefield { controller: A },
                    program: Program::land(
                        std::collections::BTreeSet::new(),
                        source
                            .abilities
                            .iter()
                            .map(|(_, id, produced)| ManaAbility {
                                id: ManaAbilityId(*id),
                                output: ManaOutput::Fixed(
                                    produced
                                        .iter()
                                        .map(|(kind, amount)| (*kind, (*amount).into()))
                                        .collect(),
                                ),
                            })
                            .collect(),
                    ),
                })
                .collect();
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                vec![A, B],
                A,
                cards,
                initial_battlefield_order,
                [
                    (A, ManaType::Black, 2),
                    (A, ManaType::Colourless, 1),
                    (B, ManaType::White, 1),
                ]
                .into_iter()
                .map(|(player, mana_type, amount)| FloatingMana {
                    player,
                    mana_type,
                    amount: amount.into(),
                })
                .collect(),
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
    )?;
    let initialization_production_present = initialized.mana_production().is_some();
    let initialization_payment_present = initialized.mana_payment().is_some();
    let mut state = initialized.into_state();
    let initial = native_state(&state, &probe.sources, "initial", None)?;
    let mut snapshots = vec![initial.snapshot.clone()];
    let mut attempts = Vec::new();
    let mut produced = Vec::new();
    let mut selected = Vec::new();
    for activation in &probe.activations {
        let source = probe
            .sources
            .iter()
            .find(|source| source.alias == activation.source)
            .ok_or(EffectError::UnknownObject)?;
        let ability = source
            .abilities
            .iter()
            .find(|(label, _, _)| *label == activation.ability)
            .ok_or(EffectError::UnknownManaAbility)?
            .1;
        let requested_source = identity(&state, source.card)?;
        let before = native_state(&state, &probe.sources, "before", None)?;
        let saved = state.clone();
        let result = spellwake_reference_tools::current_capture::advance(
            &state,
            GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: GameCommand::ActivateMana {
                    source: requested_source,
                    ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(
                        ability,
                    )),
                },
            },
        );
        let input_unchanged = state == saved;
        let (candidate_base, production, payment_receipt_present, error) = match result {
            Ok(candidate) => {
                let CandidateBase::Advance(base) = candidate.base() else {
                    return Err(EffectError::InconsistentProgram.into());
                };
                let base = Some(base.into());
                let production = candidate.mana_production().map(ProductionView::from);
                let payment = candidate.mana_payment().is_some();
                state = candidate.into_state();
                (base, production, payment, None)
            }
            Err(error) => (None, None, false, Some(format!("{error:?}"))),
        };
        let accepted = error.is_none();
        let after = native_state(&state, &probe.sources, activation.label, Some(accepted))?;
        snapshots.push(after.snapshot.clone());
        produced.push(
            production
                .as_ref()
                .map(|record| record.produced.clone())
                .unwrap_or_else(|| amounts(&BTreeMap::new())),
        );
        selected.push(activation.ability);
        attempts.push(Attempt {
            label: activation.label,
            actor: seat(A),
            source_alias: activation.source,
            requested_source: format!("{requested_source:?}"),
            requested_ability: spellwake_reference_tools::mana_ref::view(
                spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(ability)),
            ),
            before,
            after,
            candidate_base,
            production,
            payment_receipt_present,
            input_unchanged,
            error,
        });
    }
    // A fresh pass observes the actual candidate record lifetime independently of
    // the shared Forge snapshots, whose controller loop is not driven here.
    let before = native_state(&state, &probe.sources, "before-pass", None)?;
    let saved = state.clone();
    let candidate = spellwake_reference_tools::current_capture::advance(
        &state,
        GameInput {
            basis: state.basis().clone(),
            actor: A,
            command: GameCommand::Pass,
        },
    )?;
    let CandidateBase::Advance(base) = candidate.base() else {
        return Err(EffectError::InconsistentProgram.into());
    };
    let followup = Followup {
        command: "Pass",
        actor: seat(A),
        before,
        after: native_state(candidate.state(), &probe.sources, "after-pass", Some(true))?,
        candidate_base: base.into(),
        production_receipt_present: candidate.mana_production().is_some(),
        payment_receipt_present: candidate.mana_payment().is_some(),
        input_unchanged: state == saved,
    };
    let selected_ability = if selected.len() == 1 {
        OneOrMany::One(selected.remove(0))
    } else {
        OneOrMany::Many(selected)
    };
    let produced = if produced.len() == 1 {
        OneOrMany::One(produced.remove(0))
    } else {
        OneOrMany::Many(produced)
    };
    Ok((
        CommonProbe {
            id: probe.id,
            source_cards: probe
                .sources
                .iter()
                .map(|source| (source.alias, source.name))
                .collect(),
            selected_ability,
            produced,
            snapshots,
        },
        NativeProbe {
            id: probe.id,
            initialization_production_present,
            initialization_payment_present,
            initial,
            attempts,
            followup,
        },
    ))
}

fn source(
    alias: &'static str,
    name: &'static str,
    card: u64,
    owner: SeatId,
    outputs: &[AbilityInput<'_>],
) -> Source {
    Source {
        alias,
        name,
        card: CardId(card),
        owner,
        abilities: outputs
            .iter()
            .map(|(label, id, produced)| (*label, *id, produced.to_vec()))
            .collect(),
    }
}
fn one(
    group: &'static str,
    id: &'static str,
    name: &'static str,
    label: &'static str,
    ability: u64,
    produced: &[(ManaType, i64)],
) -> Probe {
    Probe {
        group,
        id,
        sources: vec![source("source", name, 1, A, &[(label, ability, produced)])],
        activations: vec![Activation {
            label: "activated",
            source: "source",
            ability: label,
        }],
    }
}
fn run(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Result<Report, GameError> {
    use ManaType::{Black, Blue, Colourless, Green, Red, White};
    let mut probes = vec![
        one("basic-types", "basic-w", "Plains", "W", 11, &[(White, 1)]),
        one("basic-types", "basic-u", "Island", "U", 23, &[(Blue, 1)]),
        one("basic-types", "basic-b", "Swamp", "B", 31, &[(Black, 1)]),
        one("basic-types", "basic-r", "Mountain", "R", 43, &[(Red, 1)]),
        one("basic-types", "basic-g", "Forest", "G", 47, &[(Green, 1)]),
        one(
            "basic-types",
            "basic-c",
            "Wastes",
            "C",
            53,
            &[(Colourless, 1)],
        ),
    ];
    for (id, label) in [("tundra-white", "W"), ("tundra-blue", "U")] {
        probes.push(Probe {
            group: "fixed-alternatives",
            id,
            sources: vec![source(
                "source",
                "Tundra",
                1,
                A,
                &[("W", 11, &[(White, 1)]), ("U", 23, &[(Blue, 1)])],
            )],
            activations: vec![Activation {
                label: "activated",
                source: "source",
                ability: label,
            }],
        });
    }
    probes.push(one(
        "fixed-multiple-output",
        "guildless-colorless-two",
        "Guildless Commons",
        "CC",
        59,
        &[(Colourless, 2)],
    ));
    probes.push(one(
        "fixed-multiple-output",
        "boros-red-white",
        "Boros Garrison",
        "RW",
        61,
        &[(Red, 1), (White, 1)],
    ));
    probes.push(Probe {
        group: "sequential-sources",
        id: "mountain-wastes",
        sources: vec![
            source("mountain", "Mountain", 1, A, &[("R", 43, &[(Red, 1)])]),
            source("wastes", "Wastes", 2, A, &[("C", 53, &[(Colourless, 1)])]),
        ],
        activations: vec![
            Activation {
                label: "mountain-activated",
                source: "mountain",
                ability: "R",
            },
            Activation {
                label: "wastes-activated",
                source: "wastes",
                ability: "C",
            },
        ],
    });
    let mut repeat = one(
        "repeat-tapped-rejected",
        "mountain-repeat",
        "Mountain",
        "R",
        43,
        &[(Red, 1)],
    );
    repeat.activations[0].label = "first-activation";
    repeat.activations.push(Activation {
        label: "rejected-repeat",
        source: "source",
        ability: "R",
    });
    probes.push(repeat);
    let mut stolen = one(
        "controller-receives-mana",
        "stolen-mountain",
        "Mountain",
        "R",
        43,
        &[(Red, 1)],
    );
    stolen.sources[0].owner = B;
    probes.push(stolen);
    let mut scenarios: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut native_observations = Vec::new();
    for (probe, index) in probes.into_iter().zip(1..) {
        let group = probe.group;
        let (common, native) = run_probe(vocabulary, index, probe)?;
        scenarios.entry(group).or_default().push(common);
        native_observations.push(native);
    }
    Ok(Report {
        schema_version: 1,
        kind: "fixed-tap-mana-abilities",
        native_driver: "game-state-initialize-advance-v1",
        program_source: "synthetic-authored-fixed-tap-noncreature-land-v1; no Oracle registry bindings",
        scope: "Shared actual source status, six-type pools, stack count, controller and accepted/rejected availability. Forge already-tapped rejection is a canPlay preflight. Native candidate receipts, exact bases, immutable failure and retained priority are separate native-only observations. No payment-time activations, real-card or full-game qualification.",
        numeric_encoding: "canonical decimal strings for rules integers",
        catalogue_bindings: 0,
        fully_qualified_cards: 0,
        full_game_comparisons: 0,
        scenarios,
        native_observations,
    })
}
fn main() -> ExitCode {
    let vocabulary = match spellwake_reference_tools::creature_types::selected_vocabulary() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("invalid qualification input: {error}");
            std::process::exit(1);
        }
    };

    let native_arguments = spellwake_reference_tools::creature_types::qualification_args()
        .unwrap_or_else(|error| {
            eprintln!("invalid qualification input: {error}");
            std::process::exit(1)
        });
    if !native_arguments.is_empty() {
        eprintln!("unsupported qualification arguments");
        std::process::exit(1);
    }

    let profile_token = "mana-abilities";
    if let Err(error) = spellwake_reference_tools::current_capture::start(
        profile_token,
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    ) {
        eprintln!("current capture source unavailable: {error}");
        return ExitCode::FAILURE;
    }
    let (response, success): (ApiResponse<Report>, bool) = match run(&vocabulary) {
        Ok(data) => (ApiResponse::Success { data }, true),
        Err(error) => {
            eprintln!("native mana ability trace failed: {error:?}");
            (
                ApiResponse::Error {
                    message: "Native mana ability trace unavailable".into(),
                    code: "native-mana-ability-error".into(),
                },
                false,
            )
        }
    };
    let response = match spellwake_reference_tools::current_capture::finish_serializable(
        profile_token,
        &response,
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("current capture incomplete: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
        eprintln!("native mana ability output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
