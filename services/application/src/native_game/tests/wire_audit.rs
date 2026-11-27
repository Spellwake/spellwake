//! Gate 2 (Engine <-> Application Wire Reachability Audit) and
//! Gate 3 (Headless 4-Player Corpus Execution + Projection Round-Trip Harness).
//!
//! These tests verify that every bound `Program` in `Profile::Current` is genuinely
//! projectable and playable across the public `services/application` player wire
//! rather than merely passing `Program::validate`.

use super::*;
use crate::player_wire::{
    DecisionKind, PlayerAction, PlayerCastMode, PlayerChoice, PlayerCounterKind, PlayerDomain,
    PlayerPayment, PlayerTarget,
};
use spellwake_card_programs::{BindingStatus, OracleId, Registry};
use spellwake_contracts::{creature_types::CreatureTypeVocabulary, names::NameRules};
use spellwake_engine::{
    activated::ExtraActivationCost,
    combat::CombatRules,
    effects::{CounterKind, Program, SeedCard},
    game::{GameId, GameState},
    land::{EntryTap, LandType},
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{BodyTarget, EntryCounterAbility, ExecutableBody, Instruction, ProgramKind},
    setup::GameSeed,
    turn::{Step, TurnStart},
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

const C: SeatId = SeatId(9);
const D: SeatId = SeatId(12);

fn viewer_c() -> Uuid {
    Uuid::from_u128(1030)
}
fn viewer_d() -> Uuid {
    Uuid::from_u128(1040)
}

fn free_payment() -> PlayerPayment {
    PlayerPayment {
        from_lots: BTreeMap::new(),
        graveyard_permission: None,
        use_granted_flashback: false,
        exile_cast_type: None,
        alternative_cost_source: None,
        spend: vec![],
        restricted_mana: vec![],
    }
}

fn load_current_registry() -> (Arc<CreatureTypeVocabulary>, Registry) {
    let vocab = vocabulary();
    let meta: PathBuf = std::env::var_os("SPELLWAKE_CATALOGUE_METADATA")
        .expect("SPELLWAKE_CATALOGUE_METADATA")
        .into();
    let input: PathBuf = std::env::var_os("SPELLWAKE_CATALOGUE_INPUT")
        .expect("SPELLWAKE_CATALOGUE_INPUT")
        .into();
    let inventory = spellwake_content::snapshot::load(&meta, &input).expect("load inventory");
    let registry = Registry::from_current_keyword_inventory(&inventory, &vocab)
        .expect("build current registry");
    (vocab, registry)
}

fn four_player_binding(state: &GameState, viewer: Uuid) -> NativeBinding {
    NativeBinding::new(
        context_id(),
        state.basis().clone(),
        vec![
            NativeSeatBinding {
                seat_id: viewer_a(),
                native_seat: A,
            },
            NativeSeatBinding {
                seat_id: viewer_b(),
                native_seat: B,
            },
            NativeSeatBinding {
                seat_id: viewer_c(),
                native_seat: C,
            },
            NativeSeatBinding {
                seat_id: viewer_d(),
                native_seat: D,
            },
        ],
        viewer,
    )
    .expect("4-player binding")
}

fn four_player_state(cards: Vec<SeedCard>, vocab: Arc<CreatureTypeVocabulary>) -> GameState {
    let order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        vec![A, B, C, D],
        A,
        cards,
        order,
        vec![],
        40.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocab,
        Arc::new(NameRules::literal_names()),
    );
    GameState::initialize_scheduled(
        GameId(990),
        context(&seed.creature_vocabulary),
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .expect("initialize 4-player game")
    .into_state()
}

fn can_seed_directly_on_battlefield(prog: &Program) -> bool {
    let ProgramKind::Permanent(def) = &prog.kind else {
        return false;
    };
    !prog
        .supertypes
        .contains(&spellwake_engine::program::Supertype::World)
        && def.attachment.is_none()
        && !prog.has_type(spellwake_engine::program::CardType::Battle)
        && !matches!(
            def.entry_tap,
            Some(
                EntryTap::AlwaysChooseColorExcept(_)
                    | EntryTap::ChooseColor
                    | EntryTap::ChooseColorExcept(_)
                    | EntryTap::ChooseCreatureType
                    | EntryTap::ChooseNonlandCardName
                    | EntryTap::ChooseOpponent
                    | EntryTap::ChoosePlayer
                    | EntryTap::ChooseTwoPlayers
                    | EntryTap::ChooseCardType
                    | EntryTap::ChooseBasicLandType
                    | EntryTap::ChooseOddOrEven
                    | EntryTap::ChooseKhansOrDragons
                    | EntryTap::ChooseOptions(_)
                    | EntryTap::Unleash
                    | EntryTap::Riot
                    | EntryTap::Tribute(_)
                    | EntryTap::ChooseModalFace
                    | EntryTap::SiegeProtector
                    | EntryTap::ChosenOpponentControls
                    | EntryTap::ReadAhead { .. }
            )
        )
}
/// Adding a new `EntryTap` variant without updating `decisions.rs` + `translate.rs`
/// fails to compile here.
fn entry_tap_wire_supported(tap: EntryTap) -> bool {
    match tap {
        // Automatic entry rules (no interactive player decision needed)
        EntryTap::Always
        | EntryTap::UnlessTwoOtherLands
        | EntryTap::UnlessTwoOrFewerOtherLands
        | EntryTap::UnlessTwoBasicLands
        | EntryTap::UnlessLandType(_, _)
        | EntryTap::UnlessTwoOpponents
        | EntryTap::UnlessOpponentLandsEight
        | EntryTap::UnlessPlayerLifeThirteenOrLess => true,
        // Interactive entry choices wired in decisions.rs + translate.rs
        EntryTap::ReadAhead { .. }
        | EntryTap::AlwaysChooseColorExcept(_)
        | EntryTap::ChooseColor
        | EntryTap::ChooseColorExcept(_)
        | EntryTap::ChooseCreatureType
        | EntryTap::ChooseNonlandCardName
        | EntryTap::ChooseOpponent
        | EntryTap::ChoosePlayer
        | EntryTap::ChooseTwoPlayers
        | EntryTap::ChooseCardType
        | EntryTap::ChooseBasicLandType
        | EntryTap::ChooseOddOrEven
        | EntryTap::ChooseKhansOrDragons
        | EntryTap::ChooseOptions(_)
        | EntryTap::Unleash
        | EntryTap::Riot
        | EntryTap::Tribute(_)
        | EntryTap::ChooseModalFace
        | EntryTap::SiegeProtector
        | EntryTap::ChosenOpponentControls
        | EntryTap::UnlessRevealHandLandType(_, _)
        | EntryTap::UnlessRevealHandSubtype { .. }
        | EntryTap::UnlessPayLife(_) => true,
    }
}

fn collect_body_counter_kinds(body: &ExecutableBody, out: &mut Vec<CounterKind>) {
    for op in &body.instructions {
        match op {
            Instruction::MutateObject(spec) => match &spec.op {
                spellwake_engine::toolbox::ObjectMutation::Counters(
                    spellwake_engine::toolbox::CounterOp::Add { kind, .. },
                ) => {
                    out.push(*kind);
                }
                spellwake_engine::toolbox::ObjectMutation::Counters(
                    spellwake_engine::toolbox::CounterOp::Remove {
                        kind: Some(kind), ..
                    }
                    | spellwake_engine::toolbox::CounterOp::Clear { kind: Some(kind) }
                    | spellwake_engine::toolbox::CounterOp::Double { kind: Some(kind) }
                    | spellwake_engine::toolbox::CounterOp::Move {
                        kind: Some(kind), ..
                    },
                ) => out.push(*kind),
                _ => {}
            },
            Instruction::ZoneOp(spec) => {
                if let spellwake_engine::toolbox::ZoneDest::Battlefield {
                    entering_counters, ..
                } = &spec.destination
                {
                    for (kind, _) in entering_counters {
                        out.push(*kind);
                    }
                }
            }
            Instruction::FixedTargetClause(inner)
            | Instruction::EmitReflexiveTrigger { body: inner }
            | Instruction::ScheduleDelayedStepTrigger { body: inner, .. }
            | Instruction::ScheduleDelayedEventTrigger { body: inner, .. }
            | Instruction::ForEach { body: inner, .. }
            | Instruction::ForEachPlayerQuery { body: inner, .. } => {
                collect_body_counter_kinds(inner, out);
            }
            Instruction::PromptChoice(spec) => {
                for branch in &spec.option_branches {
                    collect_body_counter_kinds(branch, out);
                }
            }
            _ => {}
        }
    }
    if let Some(modes) = body.triggered_modes() {
        for opt in &modes.options {
            collect_body_counter_kinds(opt, out);
        }
    }
}

fn audit_program_wire_static(name: &str, program: &Program, failures: &mut Vec<String>) {
    if let ProgramKind::Permanent(def) = &program.kind
        && let Some(tap) = def.entry_tap
        && !entry_tap_wire_supported(tap)
    {
        failures.push(format!("{name}: unsupported EntryTap::{tap:?}"));
    }
    let mut counter_kinds = Vec::new();
    for ec in &program.entry_counter_abilities {
        let kind = match *ec {
            EntryCounterAbility::FixedNamed { kind, .. }
            | EntryCounterAbility::DynamicCounter { kind, .. }
            | EntryCounterAbility::ConditionalCounter { kind, .. } => kind,
            EntryCounterAbility::Devour(_)
            | EntryCounterAbility::ChosenXPlusOne
            | EntryCounterAbility::FixedPlusOne(_)
            | EntryCounterAbility::Sunburst
            | EntryCounterAbility::Bloodthirst(_)
            | EntryCounterAbility::BloodthirstX
            | EntryCounterAbility::MorbidPlusOne(_)
            | EntryCounterAbility::ControlledAndGraveyardSubtypePlusOne(_)
            | EntryCounterAbility::GraveyardControlledSubtypePlusOne(_)
            | EntryCounterAbility::KickedOpposingNonbasicLands(_)
            | EntryCounterAbility::Amplify(_) => CounterKind::PlusOne,
            EntryCounterAbility::FixedMinusOne(_) => CounterKind::MinusOne,
            EntryCounterAbility::FixedCharge(_) | EntryCounterAbility::ChargePerKicker(_) => {
                CounterKind::Charge
            }
            EntryCounterAbility::FixedLoyalty(_) => CounterKind::Loyalty,
            EntryCounterAbility::SuspectForGreatestCreatureCount => CounterKind::Suspect,
        };
        counter_kinds.push(kind);
    }
    if let ProgramKind::Spell(sp) = &program.kind {
        collect_body_counter_kinds(&sp.body, &mut counter_kinds);
        if let Some(modes) = &sp.modes {
            for mode in &modes.additional {
                collect_body_counter_kinds(mode, &mut counter_kinds);
            }
        }
    }
    for trig in &program.triggered {
        collect_body_counter_kinds(&trig.body, &mut counter_kinds);
    }
    for act in &program.activated {
        collect_body_counter_kinds(&act.body, &mut counter_kinds);
    }
    fn collect_extra_cost_counters(cost: &ExtraActivationCost, out: &mut Vec<CounterKind>) {
        match cost {
            ExtraActivationCost::RemovePermanentCounters {
                kind: Some(kind), ..
            }
            | ExtraActivationCost::PutPermanentCounters { kind, .. }
            | ExtraActivationCost::SourceCounter { kind, .. } => out.push(*kind),
            ExtraActivationCost::Either(a, b) => {
                collect_extra_cost_counters(a, out);
                collect_extra_cost_counters(b, out);
            }
            _ => {}
        }
    }
    for costs in program
        .extra_activation_costs
        .values()
        .chain(std::iter::once(&program.extra_cast_costs))
    {
        for cost in costs {
            collect_extra_cost_counters(cost, &mut counter_kinds);
        }
    }
    for kind in counter_kinds {
        if PlayerCounterKind::try_from(kind).is_err() {
            failures.push(format!("{name}: unprojectable CounterKind::{kind:?}"));
        }
    }
    if let Some(split) = &program.split {
        audit_program_wire_static(name, &split.right, failures);
    }
    if let Some(adv) = &program.adventure {
        audit_program_wire_static(name, adv, failures);
    }
    if let Some(back) = &program.transform_back {
        audit_program_wire_static(name, back, failures);
    }
    if let Some(back) = &program.modal_back {
        audit_program_wire_static(name, back, failures);
    }
    if let Some(back) = &program.flip_back {
        audit_program_wire_static(name, back, failures);
    }
    if let Some(proto) = &program.prototype {
        audit_program_wire_static(name, proto, failures);
    }
    if let Some(prep) = &program.prepare_spell {
        audit_program_wire_static(name, prep, failures);
    }
}

#[test]
#[ignore = "uses saved dated Scryfall bulk corpus and creature vocabulary"]
fn gate2_wire_reachability_audit_all_current_bound_programs() {
    let (vocab, registry) = load_current_registry();
    let mut bound: Vec<(String, Program)> = Vec::new();
    for entry in &registry.coverage().cards {
        if matches!(entry.binding, BindingStatus::Partial { .. })
            && let Some(oid_str) = &entry.oracle_id
            && let Ok(oid) = OracleId::try_from(oid_str.clone())
            && let Some(bp) = registry.resolve(&oid)
        {
            bound.push((entry.name.clone(), bp.program().clone()));
        }
    }
    let total_bound = bound.len();
    assert!(
        total_bound > 0,
        "expected bound programs in Profile::Current"
    );

    // 1. Static exhaustive wire audit across every single bound Program + subfaces
    let mut static_failures = Vec::new();
    for (name, program) in &bound {
        audit_program_wire_static(name, program, &mut static_failures);
    }

    // 2. Full-corpus snapshot projection audit in 4-player games (batches of 64 cards)
    let mut projection_failures = Vec::new();
    let mut projected_cards = 0usize;
    for chunk in bound.chunks(64) {
        let mut cards = Vec::with_capacity(chunk.len() + 4);
        for (idx, seat) in [A, B, C, D].into_iter().enumerate() {
            cards.push(SeedCard {
                card: CardId(10_000 + idx as u64),
                owner: seat,
                destination: ObjectDestination::LibraryTop,
                program: Program::land(BTreeSet::from([LandType::Forest]), vec![]),
            });
        }
        for (i, (_name, prog)) in chunk.iter().enumerate() {
            let dest = if can_seed_directly_on_battlefield(prog) {
                ObjectDestination::Battlefield { controller: A }
            } else {
                ObjectDestination::Hand
            };
            cards.push(SeedCard {
                card: CardId(1 + i as u64),
                owner: A,
                destination: dest,
                program: prog.clone(),
            });
        }
        let state = four_player_state(cards, Arc::clone(&vocab));
        for viewer in [viewer_a(), viewer_b()] {
            let mut sc = NativeScope::new(&state, four_player_binding(&state, viewer))
                .expect("4-player scope");
            if let Err(err) = super::projection::project_test(&mut sc) {
                for (name, prog) in chunk {
                    let single_dest = if can_seed_directly_on_battlefield(prog) {
                        ObjectDestination::Battlefield { controller: A }
                    } else {
                        ObjectDestination::Hand
                    };
                    let single_state = four_player_state(
                        vec![SeedCard {
                            card: CardId(1),
                            owner: A,
                            destination: single_dest,
                            program: prog.clone(),
                        }],
                        Arc::clone(&vocab),
                    );
                    let mut single_scope =
                        NativeScope::new(&single_state, four_player_binding(&single_state, viewer))
                            .expect("single scope");
                    if let Err(single_err) = super::projection::project_test(&mut single_scope) {
                        projection_failures.push(format!("{name}: {single_err:?}"));
                    }
                }
                if projection_failures.is_empty() {
                    let detail = match &err {
                        NativeProjectionError::Bridge(bridge) => {
                            format!(
                                "Bridge({bridge}, native={:?}, pending={:?})",
                                bridge.native_error(),
                                state.pending_decision().map(|d| d.frame())
                            )
                        }
                        other => format!("{other:?}"),
                    };
                    let sample_names: Vec<&str> =
                        chunk.iter().take(6).map(|(n, _)| n.as_str()).collect();
                    projection_failures
                        .push(format!("batch error ({sample_names:?}...): {detail}"));
                }
            }
        }
        projected_cards += chunk.len();
    }

    eprintln!("=== GATE 2: ENGINE <-> APPLICATION WIRE REACHABILITY AUDIT ===");
    eprintln!(
        "Total Profile::Current bound programs audited: {total_bound} (projected: {projected_cards})"
    );
    eprintln!(
        "Static wire failures: {} | Projection failures: {}",
        static_failures.len(),
        projection_failures.len()
    );
    for sample in static_failures.iter().take(20) {
        eprintln!("  STATIC FAIL: {sample}");
    }
    for sample in projection_failures.iter().take(20) {
        eprintln!("  PROJ FAIL:   {sample}");
    }
    assert!(
        static_failures.is_empty(),
        "Gate 2 static wire reachability failures ({}): {:?}",
        static_failures.len(),
        &static_failures[..static_failures.len().min(10)]
    );
    assert!(
        projection_failures.is_empty(),
        "Gate 2 full-corpus projection failures ({}): {:?}",
        projection_failures.len(),
        &projection_failures[..projection_failures.len().min(10)]
    );
}

fn auto_reply_for_decision(dec: &crate::player_wire::PlayerDecision) -> Option<PlayerChoice> {
    match &dec.domain {
        PlayerDomain::Options { options, .. } => options.first().map(|opt| PlayerChoice::Option {
            option: opt.handle.clone(),
        }),
        PlayerDomain::Targets { targets } => targets
            .first()
            .cloned()
            .map(|target| PlayerChoice::Target { target }),
        PlayerDomain::TargetList {
            targets, minimum, ..
        } => {
            let min = minimum.as_rules_integer().to_usize().unwrap_or(0);
            if targets.len() >= min {
                Some(PlayerChoice::TargetList {
                    targets: targets.iter().take(min).cloned().collect(),
                })
            } else {
                None
            }
        }
        PlayerDomain::Objects {
            objects, minimum, ..
        } => {
            let min = minimum.as_rules_integer().to_usize().unwrap_or(0);
            if objects.len() >= min {
                Some(PlayerChoice::Objects {
                    objects: objects.iter().take(min).cloned().collect(),
                })
            } else {
                None
            }
        }
        PlayerDomain::Ordering { objects, .. } => Some(PlayerChoice::Ordering {
            ordered: objects.clone(),
        }),
        PlayerDomain::Number { minimum, .. } => Some(PlayerChoice::Number {
            value: minimum.clone(),
        }),
        PlayerDomain::LibraryTop { inspected } => {
            let handles: Vec<_> = inspected.iter().map(|o| o.handle.clone()).collect();
            match dec.kind {
                DecisionKind::Scry => Some(PlayerChoice::Scry {
                    top: handles,
                    bottom: vec![],
                }),
                DecisionKind::Surveil => Some(PlayerChoice::Surveil {
                    top: handles,
                    graveyard: vec![],
                }),
                _ => None,
            }
        }
        PlayerDomain::LibrarySearch {
            eligible,
            required,
            mode,
            entry_color_offers,
            entry_reveal_offers,
            ..
        } => match mode {
            crate::player_wire::PlayerSearchMode::BattlefieldOrHand => {
                if *required {
                    eligible
                        .first()
                        .map(|opt| PlayerChoice::SearchToDestination {
                            object: Some(opt.handle.clone()),
                            battlefield: false,
                        })
                } else {
                    Some(PlayerChoice::SearchToDestination {
                        object: None,
                        battlefield: false,
                    })
                }
            }
            crate::player_wire::PlayerSearchMode::HandGraveyard => {
                let hand = eligible.first().map(|o| o.handle.clone());
                let graveyard = eligible.get(1).map(|o| o.handle.clone());
                Some(PlayerChoice::SearchHandGraveyard { hand, graveyard })
            }
            _ => {
                let chosen = if *required {
                    eligible
                        .first()
                        .map(|o| vec![o.handle.clone()])
                        .unwrap_or_default()
                } else {
                    vec![]
                };
                if !entry_color_offers.is_empty() || !entry_reveal_offers.is_empty() {
                    Some(PlayerChoice::SearchWithEntryChoices {
                        objects: chosen,
                        colors: vec![],
                        reveals: vec![],
                    })
                } else {
                    Some(PlayerChoice::Objects { objects: chosen })
                }
            }
        },
        PlayerDomain::ImpulseCast { .. }
        | PlayerDomain::CascadeCast { .. }
        | PlayerDomain::MadnessCast { .. }
        | PlayerDomain::SuspendedCast { .. }
        | PlayerDomain::ReboundCast { .. } => Some(PlayerChoice::Decline {}),
        PlayerDomain::Payment { allow_decline, .. } if *allow_decline => {
            Some(PlayerChoice::Decline {})
        }
        PlayerDomain::TriggerModes { minimum, modes, .. } => {
            let min = usize::from(*minimum);
            let mut chosen = Vec::new();
            for m in modes {
                if chosen.len() >= min {
                    break;
                }
                if m.target_required {
                    if let Some(t) = m.targets.first() {
                        chosen.push(PlayerCastMode {
                            mode: m.index,
                            target: Some(t.clone()),
                            targets: vec![],
                        });
                    }
                } else {
                    chosen.push(PlayerCastMode {
                        mode: m.index,
                        target: None,
                        targets: vec![],
                    });
                }
            }
            (chosen.len() >= min).then_some(PlayerChoice::TriggerModes { modes: chosen })
        }
        _ => None,
    }
}

fn is_entropy_exhausted(err: &NativeBridgeError) -> bool {
    matches!(
        err.native_error(),
        Some(
            spellwake_engine::game::GameError::Entropy(_)
                | spellwake_engine::game::GameError::Effect(
                    spellwake_engine::effects::EffectError::Window(
                        spellwake_engine::stack_window::StackWindowError::Objects(
                            spellwake_engine::object_identity::ObjectIdentityError::Entropy(_)
                        )
                    )
                )
        )
    )
}

fn format_bridge_err(err: &NativeBridgeError) -> String {
    match err.native_error() {
        Some(native) => format!("{err} (native={native:?})"),
        None => format!("{err}"),
    }
}

fn seat_viewer(seat: SeatId) -> Uuid {
    match seat {
        A => viewer_a(),
        B => viewer_b(),
        C => viewer_c(),
        D => viewer_d(),
        _ => viewer_a(),
    }
}

#[test]
#[ignore = "uses saved dated Scryfall bulk corpus and creature vocabulary"]
fn gate3_headless_four_player_corpus_execution_smoke() {
    let (vocab, registry) = load_current_registry();

    let mut lands = Vec::new();
    let mut creatures = Vec::new();
    let mut artifacts_ench = Vec::new();
    let mut untargeted_spells = Vec::new();
    let mut targeted_spells = Vec::new();
    let mut scry_surveil_shock = Vec::new();

    for entry in &registry.coverage().cards {
        if !matches!(entry.binding, BindingStatus::Partial { .. }) {
            continue;
        }
        let Some(oid_str) = &entry.oracle_id else {
            continue;
        };
        let Ok(oid) = OracleId::try_from(oid_str.clone()) else {
            continue;
        };
        let Some(bp) = registry.resolve(&oid) else {
            continue;
        };
        let p = bp.program().clone();
        let has_scry_or_shock = match &p.kind {
            ProgramKind::Permanent(def) => {
                matches!(def.entry_tap, Some(EntryTap::UnlessPayLife(_)))
            }
            ProgramKind::Spell(sp) => sp.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::MutatePlayer(spec)
                        if matches!(
                            spec.op,
                            spellwake_engine::toolbox::PlayerMutation::LibraryTop(
                                spellwake_engine::toolbox::LibraryTopAction::Scry(_)
                                    | spellwake_engine::toolbox::LibraryTopAction::Surveil(_)
                            )
                        )
                )
            }),
        };
        if has_scry_or_shock && scry_surveil_shock.len() < 80 {
            scry_surveil_shock.push((entry.name.clone(), p));
        } else {
            match &p.kind {
                ProgramKind::Permanent(_)
                    if p.has_type(spellwake_engine::program::CardType::Land)
                        && lands.len() < 80 =>
                {
                    lands.push((entry.name.clone(), p));
                }
                ProgramKind::Permanent(_)
                    if p.has_type(spellwake_engine::program::CardType::Creature)
                        && creatures.len() < 80 =>
                {
                    creatures.push((entry.name.clone(), p));
                }
                ProgramKind::Permanent(_) if artifacts_ench.len() < 60 => {
                    artifacts_ench.push((entry.name.clone(), p));
                }
                ProgramKind::Spell(sp)
                    if sp.body.target == BodyTarget::None && untargeted_spells.len() < 80 =>
                {
                    untargeted_spells.push((entry.name.clone(), p));
                }
                ProgramKind::Spell(_) if targeted_spells.len() < 80 => {
                    targeted_spells.push((entry.name.clone(), p));
                }
                _ => {}
            }
        }
    }

    let sample: Vec<(String, Program)> = scry_surveil_shock
        .into_iter()
        .chain(lands)
        .chain(creatures)
        .chain(artifacts_ench)
        .chain(untargeted_spells)
        .chain(targeted_spells)
        .collect();

    let mut tested = 0usize;
    let mut actions_executed = 0usize;
    let mut decisions_resolved = 0usize;
    let mut hard_failures: Vec<String> = Vec::new();

    for (name, mut candidate) in sample {
        tested += 1;
        let is_land = matches!(&candidate.kind, ProgramKind::Permanent(_))
            && candidate.has_type(spellwake_engine::program::CardType::Land);
        let has_cast_cost = match &candidate.kind {
            ProgramKind::Permanent(def) => def.casting_cost.is_some(),
            ProgramKind::Spell(_) => true,
        };
        let mut can_free_cast = !is_land
            && has_cast_cost
            && candidate.additional_life_cost.is_none()
            && !candidate.sacrifice_creature_cast_cost
            && !candidate.discard_cast_cost
            && candidate.graveyard_creature_exile_cost == 0
            && candidate.hybrid_symbols.is_empty()
            && candidate.extra_cast_costs.is_empty();
        let simple_target = match &candidate.kind {
            ProgramKind::Spell(sp) if sp.modes.is_none() => match sp.body.target {
                BodyTarget::None => Some(None),
                BodyTarget::SingleCreature
                | BodyTarget::SinglePermanent
                | BodyTarget::SingleCreatureOrPlayer
                | BodyTarget::SingleAnyTarget => Some(Some(CardId(20))),
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent => Some(Some(CardId(0))),
                _ => None,
            },
            ProgramKind::Permanent(def) if !is_land && def.attachment.is_none() => Some(None),
            _ => None,
        };
        if can_free_cast {
            let mut zeroed = candidate.clone();
            match &mut zeroed.kind {
                ProgramKind::Permanent(def) => def.casting_cost = Some(ManaCost::free()),
                ProgramKind::Spell(sp) => sp.cost = ManaCost::free(),
            }
            if zeroed.validate(&vocab).is_ok() {
                candidate = zeroed;
            } else {
                can_free_cast = false;
            }
        }

        let mut cards = vec![
            SeedCard {
                card: CardId(1),
                owner: A,
                destination: ObjectDestination::Hand,
                program: candidate,
            },
            SeedCard {
                card: CardId(20),
                owner: B,
                destination: ObjectDestination::Battlefield { controller: B },
                program: Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            },
            SeedCard {
                card: CardId(21),
                owner: A,
                destination: ObjectDestination::Battlefield { controller: A },
                program: Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
            },
            SeedCard {
                card: CardId(22),
                owner: B,
                destination: ObjectDestination::Battlefield { controller: B },
                program: Program::artifact(None, vec![]),
            },
            SeedCard {
                card: CardId(23),
                owner: A,
                destination: ObjectDestination::Graveyard,
                program: Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            },
        ];
        for (seat_idx, seat) in [A, B, C, D].into_iter().enumerate() {
            for k in 0..5u64 {
                cards.push(SeedCard {
                    card: CardId(100 + seat_idx as u64 * 10 + k),
                    owner: seat,
                    destination: ObjectDestination::LibraryTop,
                    program: Program::land(BTreeSet::from([LandType::Forest]), vec![]),
                });
            }
            cards.push(SeedCard {
                card: CardId(200 + seat_idx as u64),
                owner: seat,
                destination: ObjectDestination::Hand,
                program: Program::land(BTreeSet::from([LandType::Island]), vec![]),
            });
        }

        let mut state = four_player_state(cards, Arc::clone(&vocab));
        for seat in [A, B, C, D] {
            let mut sc = NativeScope::new(&state, four_player_binding(&state, seat_viewer(seat)))
                .expect("scope");
            if let Err(err) = super::projection::project_test(&mut sc) {
                hard_failures.push(format!("{name}: initial snapshot seat {seat:?}: {err:?}"));
                continue;
            }
        }

        let mut played_candidate = false;
        if is_land {
            let mut sc =
                NativeScope::new(&state, four_player_binding(&state, viewer_a())).expect("scope");
            let card_obj = object(&state, 1);
            let handle = sc.issue(NativeReference::Object(card_obj)).expect("handle");
            let cmd = command(&sc, PlayerAction::PlayLand { source: handle });
            match sc.propose(&cmd) {
                Ok(prepared) => {
                    state = prepared.into_parts().3.into_state();
                    played_candidate = true;
                    actions_executed += 1;
                }
                Err(err) => {
                    hard_failures.push(format!("{name}: PlayLand rejected: {err:?}"));
                }
            }
        } else if can_free_cast && let Some(tgt_opt) = simple_target {
            let mut sc =
                NativeScope::new(&state, four_player_binding(&state, viewer_a())).expect("scope");
            let card_obj = object(&state, 1);
            let handle = sc.issue(NativeReference::Object(card_obj)).expect("handle");
            let action = match tgt_opt {
                None => PlayerAction::CastUntargeted {
                    source: handle,
                    payment: Some(free_payment()),
                },
                Some(CardId(0)) => PlayerAction::CastTargeted {
                    source: handle,
                    target: PlayerTarget::Seat {
                        seat_id: viewer_b(),
                    },
                    payment: Some(free_payment()),
                },
                Some(cid) => {
                    let tgt_obj = object(&state, cid.0);
                    let tgt_handle = sc.issue(NativeReference::Object(tgt_obj)).expect("tgt");
                    PlayerAction::CastTargeted {
                        source: handle,
                        target: PlayerTarget::Object { handle: tgt_handle },
                        payment: Some(free_payment()),
                    }
                }
            };
            let cmd = command(&sc, action);
            match sc.propose(&cmd) {
                Ok(prepared) => {
                    state = prepared.into_parts().3.into_state();
                    played_candidate = true;
                    actions_executed += 1;
                }
                Err(NativeBridgeError::Native(_)) => {}
                Err(err) => {
                    hard_failures.push(format!("{name}: Cast bridge error: {err:?}"));
                }
            }
        }

        if played_candidate {
            for _step in 0..32 {
                if state.outcome().is_some() {
                    break;
                }
                if let Some(pending) = state.pending_decision() {
                    let chooser = pending.chooser();
                    let mut sc =
                        NativeScope::new(&state, four_player_binding(&state, seat_viewer(chooser)))
                            .expect("scope");
                    let snap = match super::projection::project_test(&mut sc) {
                        Ok(s) => s,
                        Err(err) => {
                            hard_failures.push(format!(
                                "{name}: decision snapshot error on {:?}: {err:?}",
                                pending.frame()
                            ));
                            break;
                        }
                    };
                    let Some(dec) = snap.decisions.first() else {
                        hard_failures.push(format!(
                            "{name}: pending decision {:?} missing from snapshot",
                            pending.frame()
                        ));
                        break;
                    };
                    let Some(choice) = auto_reply_for_decision(dec) else {
                        break;
                    };
                    let cmd = command(
                        &sc,
                        PlayerAction::Choose {
                            decision: dec.decision.clone(),
                            response: choice,
                        },
                    );
                    match sc.propose(&cmd) {
                        Ok(prepared) => {
                            state = prepared.into_parts().3.into_state();
                            decisions_resolved += 1;
                        }
                        Err(err) if is_entropy_exhausted(&err) => {
                            match state.supply_entropy(state.basis(), vec![42; 64]) {
                                Ok(cand) => state = cand.into_state(),
                                Err(err) => {
                                    hard_failures.push(format!(
                                        "{name}: entropy supply error on {:?}: {err:?}",
                                        dec.kind
                                    ));
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            hard_failures.push(format!(
                                "{name}: decision reply error on {:?}: {}",
                                dec.kind,
                                format_bridge_err(&err)
                            ));
                            break;
                        }
                    }
                } else if let spellwake_engine::stack_window::WindowPhase::Priority(
                    spellwake_engine::priority::PriorityPhase::Ready { holder },
                ) = state.execution().window().phase()
                {
                    if state.execution().window().entries().is_empty() {
                        for seat in [A, B, C, D] {
                            let mut sc = NativeScope::new(
                                &state,
                                four_player_binding(&state, seat_viewer(seat)),
                            )
                            .expect("scope");
                            if let Err(err) = super::projection::project_test(&mut sc) {
                                hard_failures.push(format!(
                                    "{name}: post-resolution snapshot seat {seat:?}: {err:?}"
                                ));
                            }
                        }
                        break;
                    }
                    let sc =
                        NativeScope::new(&state, four_player_binding(&state, seat_viewer(holder)))
                            .expect("scope");
                    let cmd = command(&sc, PlayerAction::Pass {});
                    match sc.propose(&cmd) {
                        Ok(prepared) => {
                            state = prepared.into_parts().3.into_state();
                        }
                        Err(err) if is_entropy_exhausted(&err) => {
                            match state.supply_entropy(state.basis(), vec![42; 64]) {
                                Ok(cand) => state = cand.into_state(),
                                Err(err) => {
                                    hard_failures.push(format!(
                                        "{name}: Pass entropy supply error: {err:?}"
                                    ));
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            hard_failures
                                .push(format!("{name}: Pass error: {}", format_bridge_err(&err)));
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
        }
    }

    eprintln!("=== GATE 3: HEADLESS 4-PLAYER CORPUS EXECUTION SMOKE ===");
    eprintln!(
        "Tested: {tested} cards | Actions executed: {actions_executed} | Decisions resolved: {decisions_resolved} | Hard failures: {}",
        hard_failures.len()
    );
    for sample in hard_failures.iter().take(20) {
        eprintln!("  EXEC FAIL: {sample}");
    }
    assert!(
        hard_failures.is_empty(),
        "Gate 3 execution/projection failures ({}): {:?}",
        hard_failures.len(),
        &hard_failures[..hard_failures.len().min(10)]
    );
}
