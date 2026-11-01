//! Actual GameState payment observations for five authored instant cost probes.
//! Printed names identify reference cost sources, not native whole-card bindings.
use serde::Serialize;
use spellwake_contracts::{ApiResponse, RulesInteger};
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    effects::{EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, GameBasis, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
    },
    mana::{FloatingMana, ManaCost, ManaPayment, ManaSpend, ManaType},
    priority::SeatId,
    stack_window::StackKind,
    targets::TargetRef,
};
use std::{collections::BTreeMap, process::ExitCode};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const SUBJECT: CardId = CardId(1);
const SOURCE: CardId = CardId(2);
type Pool = BTreeMap<&'static str, RulesInteger>;

#[derive(Serialize)]
struct CostView {
    generic: RulesInteger,
    required: Pool,
}
impl From<&ManaCost> for CostView {
    fn from(cost: &ManaCost) -> Self {
        Self {
            generic: cost.generic.clone(),
            required: ManaType::ALL
                .into_iter()
                .map(|mana_type| {
                    (
                        symbol(mana_type),
                        cost.required
                            .get(&mana_type)
                            .cloned()
                            .unwrap_or_else(|| 0.into()),
                    )
                })
                .collect(),
        }
    }
}
#[derive(Serialize)]
struct SpendView {
    mana_type: &'static str,
    amount: RulesInteger,
}
#[derive(Serialize)]
struct BasisView {
    game: String,
    context: String,
    epoch: u64,
    revision: u64,
}
impl From<&GameBasis> for BasisView {
    fn from(basis: &GameBasis) -> Self {
        Self {
            game: format!("{:?}", basis.game),
            context: format!("{:?}", basis.context),
            epoch: basis.epoch,
            revision: basis.revision,
        }
    }
}
#[derive(Serialize)]
struct PaymentView {
    source: String,
    payer: u32,
    cost: CostView,
    spend: Vec<SpendView>,
}
#[derive(Serialize)]
struct AdmissionView {
    stack_entry: String,
    kind: &'static str,
    source_card: u64,
    controller: u32,
    source_before: String,
    source_after: String,
    source_zone_before: &'static str,
    source_zone_after: &'static str,
    old_incarnation_present: bool,
    stack_size_before: usize,
    stack_size_after: usize,
}
#[derive(Serialize)]
struct Observation {
    id: &'static str,
    reference_cost_source: &'static str,
    printed_reference_cost: &'static str,
    native_command: &'static str,
    requested_spend: Vec<SpendView>,
    initial_pool: Pool,
    final_pool: Pool,
    other_pool_before: Pool,
    other_pool_after: Pool,
    basis_before: BasisView,
    candidate_base: BasisView,
    basis_after: BasisView,
    receipt: PaymentView,
    admission: AdmissionView,
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
    negative_reference_scripts_compared_as_casts: u32,
    scenarios: Vec<Observation>,
}

struct Probe {
    id: &'static str,
    reference: &'static str,
    printed_cost: &'static str,
    cost: ManaCost,
    pool: [i64; 6],
    spend: Vec<(ManaType, i64)>,
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

fn pool(state: &GameState, player: SeatId) -> Pool {
    ManaType::ALL
        .into_iter()
        .map(|mana_type| (symbol(mana_type), state.execution().mana(player, mana_type)))
        .collect()
}

fn identity(state: &GameState, card: CardId) -> Result<CardObjectId, GameError> {
    state
        .execution()
        .window()
        .cards()
        .current_identity(card)
        .ok_or(EffectError::UnknownCard.into())
}

fn spend_view(spend: &[ManaSpend]) -> Vec<SpendView> {
    spend
        .iter()
        .map(|entry| SpendView {
            mana_type: symbol(entry.mana_type),
            amount: entry.amount.clone(),
        })
        .collect()
}

fn run_probe(index: u128, probe: Probe) -> Result<Observation, GameError> {
    let initialized = GameState::initialize(
        GameId(index),
        GameContext::new(
            "native-mana-payment-v1".into(),
            "MagicCompRules-20260925".into(),
            "synthetic-no-catalogue-binding".into(),
            "authored-instant-placeholder-v1".into(),
            probe.id.into(),
        )?,
        vec![A, B],
        A,
        vec![
            SeedCard {
                card: SUBJECT,
                owner: B,
                destination: CardDestination::Battlefield { controller: B },
                program: Program::VanillaCreature {
                    power: 1.into(),
                    toughness: 3.into(),
                },
            },
            SeedCard {
                card: SOURCE,
                owner: A,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: probe.cost,
                    instructions: vec![Instruction::ReturnToOwnersHand],
                },
            },
        ],
        ManaType::ALL
            .into_iter()
            .zip(probe.pool)
            .map(|(mana_type, amount)| FloatingMana {
                player: A,
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    )?;
    if initialized.mana_payment().is_some() {
        return Err(EffectError::InconsistentProgram.into());
    }
    let state = initialized.into_state();
    let source_before = identity(&state, SOURCE)?;
    let source = state
        .execution()
        .window()
        .cards()
        .get(source_before)
        .ok_or(EffectError::UnknownCard)?;
    if source.zone() != CardZone::Hand(A) {
        return Err(EffectError::NotOwnHand.into());
    }
    let target = TargetRef::Card(identity(&state, SUBJECT)?);
    let payment = ManaPayment {
        spend: probe
            .spend
            .into_iter()
            .map(|(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    };
    let requested_spend = spend_view(&payment.spend);
    let (native_command, command) = if probe.id == "zero-cost" {
        (
            "Cast",
            GameCommand::Cast {
                spell: source_before,
                target,
            },
        )
    } else {
        (
            "CastPaid",
            GameCommand::CastPaid {
                spell: source_before,
                target,
                payment,
            },
        )
    };
    let candidate = state.advance(GameInput {
        basis: state.basis().clone(),
        actor: A,
        command,
    })?;
    let receipt = candidate
        .mana_payment()
        .ok_or(EffectError::InconsistentProgram)?;
    let CandidateBase::Advance(base) = candidate.base() else {
        return Err(EffectError::InconsistentProgram.into());
    };
    let after = candidate.state();
    let entry = after
        .execution()
        .window()
        .entries()
        .last()
        .ok_or(EffectError::InconsistentProgram)?;
    let StackKind::CardSpell(source_after) = entry.kind() else {
        return Err(EffectError::InconsistentProgram.into());
    };
    let admitted = after
        .execution()
        .window()
        .cards()
        .get(*source_after)
        .ok_or(EffectError::UnknownCard)?;
    if admitted.zone() != CardZone::Stack {
        return Err(EffectError::InconsistentProgram.into());
    }
    Ok(Observation {
        id: probe.id,
        reference_cost_source: probe.reference,
        printed_reference_cost: probe.printed_cost,
        native_command,
        requested_spend,
        initial_pool: pool(&state, A),
        final_pool: pool(after, A),
        other_pool_before: pool(&state, B),
        other_pool_after: pool(after, B),
        basis_before: state.basis().into(),
        candidate_base: base.into(),
        basis_after: after.basis().into(),
        receipt: PaymentView {
            source: format!("{:?}", receipt.source),
            payer: receipt.payer.0,
            cost: (&receipt.cost).into(),
            spend: spend_view(&receipt.spend),
        },
        admission: AdmissionView {
            stack_entry: format!("{:?}", entry.id()),
            kind: "CardSpell",
            source_card: admitted.card().0,
            controller: entry.controller().0,
            source_before: format!("{source_before:?}"),
            source_after: format!("{source_after:?}"),
            source_zone_before: "HAND",
            source_zone_after: "STACK",
            old_incarnation_present: after
                .execution()
                .window()
                .cards()
                .get(source_before)
                .is_some(),
            stack_size_before: state.execution().window().entries().len(),
            stack_size_after: after.execution().window().entries().len(),
        },
    })
}

fn run() -> Result<Report, GameError> {
    use ManaType::{Blue, Colourless, Red, White};
    let probes = vec![
        Probe {
            id: "generic-colorless",
            reference: "Sol Ring",
            printed_cost: "{1}",
            cost: ManaCost::generic(1.into()),
            pool: [0, 0, 0, 1, 0, 1],
            spend: vec![(Colourless, 1)],
        },
        Probe {
            id: "generic-colored",
            reference: "Sol Ring",
            printed_cost: "{1}",
            cost: ManaCost::generic(1.into()),
            pool: [0, 0, 0, 1, 0, 1],
            spend: vec![(Red, 1)],
        },
        Probe {
            id: "mixed-generic",
            reference: "Warleader's Helix",
            printed_cost: "{2}{R}{W}",
            cost: ManaCost {
                generic: 2.into(),
                required: BTreeMap::from([(Red, 1.into()), (White, 1.into())]),
            },
            pool: [1, 1, 0, 1, 1, 1],
            spend: vec![(Red, 1), (White, 1), (Colourless, 1), (Blue, 1)],
        },
        Probe {
            id: "true-colorless-success",
            reference: "Spatial Contortion",
            printed_cost: "{1}{C}",
            cost: ManaCost {
                generic: 1.into(),
                required: BTreeMap::from([(Colourless, 1.into())]),
            },
            pool: [0, 0, 0, 1, 0, 1],
            spend: vec![(Red, 1), (Colourless, 1)],
        },
        Probe {
            id: "zero-cost",
            reference: "Ornithopter",
            printed_cost: "{0}",
            cost: ManaCost::free(),
            pool: [0, 0, 1, 0, 0, 0],
            spend: vec![],
        },
    ];
    let scenarios = probes
        .into_iter()
        .zip(1..)
        .map(|(probe, index)| run_probe(index, probe))
        .collect::<Result<_, _>>()?;
    Ok(Report {
        schema_version: 1,
        kind: "restricted-mana-payment",
        native_driver: "game-state-initialize-advance-v1",
        program_source: "synthetic-authored-instant-placeholder-v1; no Oracle registry bindings",
        scope: "Five exact fixed-cost payments from unrestricted already-floating mana. Native authored instant admission and candidate receipts; Forge payment primitives only. Printed reference names identify costs, not native card semantics. No mana production, restrictions, general casting, negative-script rollback parity or full cards/games.",
        numeric_encoding: "canonical decimal strings for rules integers",
        catalogue_bindings: 0,
        fully_qualified_cards: 0,
        full_game_comparisons: 0,
        negative_reference_scripts_compared_as_casts: 0,
        scenarios,
    })
}

fn main() -> ExitCode {
    let (response, success): (ApiResponse<Report>, bool) = match run() {
        Ok(data) => (ApiResponse::Success { data }, true),
        Err(error) => {
            eprintln!("native mana payment trace failed: {error:?}");
            (
                ApiResponse::Error {
                    message: "Native mana payment trace unavailable".into(),
                    code: "native-mana-payment-error".into(),
                },
                false,
            )
        }
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
        eprintln!("native mana payment output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
