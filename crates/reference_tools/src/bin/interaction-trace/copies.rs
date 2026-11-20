//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! A distinct copy-aware projection profile; it shares source loading and native
//! execution with the original instant baseline, whose recorded schema stays fixed.
use super::{
    Aliases, CardView, NATIVE_DRIVER, cast, identity, initialize, registered_seed, resolve,
};
use serde::Serialize;
use spellwake_card_programs::{BOLT, COUNTERSPELL, LION, Registry, TWINCAST, UNSUMMON};
use spellwake_engine::{
    effects::{CopyTargetChoice, EffectError, EffectWindow},
    game::{GameError, GameId},
    mana::{FloatingMana, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    priority::SeatId,
    stack_window::{StackEntryId, StackKind},
    targets::TargetRef,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
#[derive(Serialize)]
struct EntryView {
    alias: &'static str,
    kind: &'static str,
    owner: &'static str,
    controller: &'static str,
    targets: Vec<&'static str>,
}
#[derive(Serialize)]
struct Observation {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,

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
pub(super) struct Report {
    schema_version: u32,
    kind: &'static str,
    registry_input: &'static str,
    registry_source_sha256: String,
    scope: &'static str,
    scenarios: Vec<Scenario>,
    fully_qualified_cards: u32,
    full_game_comparisons: u32,
    native_driver: &'static str,
}
fn seat(value: SeatId) -> Result<&'static str, EffectError> {
    match value {
        A => Ok("A"),
        B => Ok("B"),
        _ => Err(EffectError::InconsistentProgram),
    }
}
fn alias(card: CardId) -> Result<&'static str, EffectError> {
    match card.0 {
        1 => Ok("bolt"),
        2 => Ok("twincast"),
        3 => Ok("counter"),
        4 => Ok("bounce"),
        5 => Ok("lion1"),
        6 => Ok("lion2"),
        _ => Err(EffectError::InconsistentProgram),
    }
}
fn observe(
    state: &EffectWindow,
    checkpoint: &'static str,
    aliases: &mut Aliases,
) -> Result<Observation, EffectError> {
    for card in state.window().cards().iter() {
        aliases.cards.insert(
            card.id(),
            alias(card.card_id().expect("source-authored card-only trace"))?,
        );
    }
    for entry in state.window().entries() {
        let name = match entry.kind() {
            StackKind::CardSpell(object) => *aliases
                .cards
                .get(object)
                .ok_or(EffectError::InconsistentProgram)?,
            StackKind::SpellCopy { .. } => "bolt-copy",
            _ => return Err(EffectError::InconsistentProgram),
        };
        aliases.stack.insert(entry.id(), name);
    }
    let mut stack = Vec::new();
    for entry in state.window().entries() {
        let mut targets = Vec::new();
        for target in entry.targets().targets() {
            let value = match target.choice().target {
                TargetRef::Object(id) => aliases.cards.get(&id),
                TargetRef::Stack(id) => aliases.stack.get(&id),
                TargetRef::Player(_) => None,
            }
            .ok_or(EffectError::InconsistentProgram)?;
            targets.push(*value);
        }
        stack.push(EntryView {
            alias: aliases
                .stack
                .get(&entry.id())
                .ok_or(EffectError::InconsistentProgram)?,
            kind: if matches!(entry.kind(), StackKind::SpellCopy { .. }) {
                "spell-copy"
            } else {
                "card-spell"
            },
            owner: seat(
                state
                    .window()
                    .spell_owner(entry)
                    .ok_or(EffectError::InconsistentProgram)?,
            )?,
            controller: seat(entry.controller())?,
            targets,
        });
    }
    let evaluated = state.evaluated_characteristics()?;
    let mut cards = Vec::new();
    for key in 1..=6 {
        let object = identity(state, key)?;
        let card = state
            .window()
            .cards()
            .get(object)
            .ok_or(EffectError::InconsistentProgram)?;
        let creature = evaluated
            .get(object)
            .and_then(|value| value.power_toughness.as_ref());
        cards.push(CardView {
            alias: alias(card.card_id().expect("source-authored card-only trace"))?,
            owner: seat(card.owner())?,
            zone: match card.zone() {
                ObjectZone::Hand(_) => "HAND",
                ObjectZone::Stack => "STACK",
                ObjectZone::Battlefield => "BATTLEFIELD",
                ObjectZone::Graveyard(_) => "GRAVEYARD",
                _ => return Err(EffectError::InconsistentProgram),
            },
            damage: state.battlefield_marked_damage(object),
            power: creature.as_ref().map(|view| view.derived.power.clone()),
            toughness: creature.map(|view| view.derived.toughness.clone()),
        });
    }
    Ok(Observation {
        capture_point: spellwake_reference_tools::capture_points::token(
            concat!(file!(), "::observe"),
            spellwake_reference_tools::current_profile::SnapshotOwner::EffectWindow(state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("current native observation point"),
        checkpoint,
        stack_bottom_first: stack,
        cards,
    })
}
fn stack_id(state: &EffectWindow, physical: u64) -> Result<StackEntryId, EffectError> {
    state.window().entries().iter().find(|entry| matches!(entry.kind(),StackKind::CardSpell(object) if state.window().cards().get(*object).is_some_and(|card|card.card_id().expect("source-authored card-only trace")==CardId(physical)))).map(|entry|entry.id()).ok_or(EffectError::InconsistentProgram)
}
fn scenario(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    name: &'static str,
    registry: &Registry,
    game: GameId,
) -> Result<Scenario, GameError> {
    let mut seeds = Vec::new();
    for (oracle, key, owner, destination) in [
        (BOLT, 1, A, ObjectDestination::Hand),
        (TWINCAST, 2, B, ObjectDestination::Hand),
        (COUNTERSPELL, 3, A, ObjectDestination::Hand),
        (UNSUMMON, 4, A, ObjectDestination::Hand),
        (LION, 5, B, ObjectDestination::Battlefield { controller: B }),
        (LION, 6, A, ObjectDestination::Battlefield { controller: A }),
    ] {
        seeds.push(registered_seed(registry, oracle, key, owner, destination)?);
    }
    let mut state = initialize(
        vocabulary,
        registry,
        game,
        "restricted-spell-copies-v3",
        seeds,
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: 1.into(),
            },
            FloatingMana {
                player: A,
                mana_type: ManaType::Blue,
                amount: 3.into(),
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Blue,
                amount: 2.into(),
            },
        ],
    )?;
    let mut aliases = Aliases::default();
    for card in state.execution().window().cards().iter() {
        aliases.cards.insert(
            card.id(),
            alias(card.card_id().expect("source-authored card-only trace"))?,
        );
    }
    let lion1 = identity(state.execution(), 5)?;
    let lion2 = identity(state.execution(), 6)?;
    state = cast(state, A, 1, TargetRef::Object(lion1))?;
    let bolt = stack_id(state.execution(), 1)?;
    let mut observations = vec![observe(state.execution(), "bolt-on-stack", &mut aliases)?];
    state = cast(state, B, 2, TargetRef::Stack(bolt))?;
    observations.push(observe(
        state.execution(),
        "twincast-on-stack",
        &mut aliases,
    )?);
    if name.contains("missing-target") {
        state = cast(state, A, 4, TargetRef::Object(lion1))?;
        observations.push(observe(state.execution(), "bounce-on-stack", &mut aliases)?);
        state = resolve(state, None)?;
        observations.push(observe(state.execution(), "after-bounce", &mut aliases)?);
    }
    let choice = if matches!(name, "copy-retain-missing-target" | "copy-target-departs") {
        CopyTargetChoice::Retain
    } else {
        CopyTargetChoice::Change(TargetRef::Object(lion2))
    };
    state = resolve(state, Some(choice))?;
    observations.push(observe(state.execution(), "after-copy", &mut aliases)?);
    if name.starts_with("counter-") {
        let target = if name == "counter-original" {
            bolt
        } else {
            state
                .execution()
                .window()
                .entries()
                .last()
                .ok_or(EffectError::InconsistentProgram)?
                .id()
        };
        state = cast(state, A, 3, TargetRef::Stack(target))?;
        observations.push(observe(
            state.execution(),
            "counter-on-stack",
            &mut aliases,
        )?);
        state = resolve(state, None)?;
        observations.push(observe(state.execution(), "after-counter", &mut aliases)?);
    } else {
        if name == "copy-target-departs" {
            state = cast(state, A, 4, TargetRef::Object(lion1))?;
            observations.push(observe(state.execution(), "bounce-on-stack", &mut aliases)?);
            state = resolve(state, None)?;
            observations.push(observe(state.execution(), "after-bounce", &mut aliases)?);
        }
        state = resolve(state, None)?;
        observations.push(observe(
            state.execution(),
            "after-copy-resolution",
            &mut aliases,
        )?);
    }
    state = resolve(state, None)?;
    observations.push(observe(state.execution(), "finished", &mut aliases)?);
    Ok(Scenario {
        scenario: name,
        observations,
    })
}
pub(super) fn run(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    registry: &Registry,
    input: &'static str,
) -> Result<Report, GameError> {
    let names = [
        "copy-redirect",
        "counter-original",
        "counter-copy",
        "copy-retain-missing-target",
        "copy-retarget-missing-target",
        "copy-target-departs",
    ];
    let scenarios = names
        .into_iter()
        .zip(101..)
        .map(|(name, id)| scenario(vocabulary, name, registry, GameId(id)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Report {
        schema_version: 3,
        kind: "restricted-spell-copies",
        registry_input: input,
        registry_source_sha256: registry.coverage().source_sha256.clone(),
        scope: "Single-target homogeneous-cost instants; explicit native copy resolution choice. Common post-SBA projections; no casting/payment/priority equivalence, permanent or ability copies, complete cards/games or performance.",
        scenarios,
        fully_qualified_cards: 0,
        full_game_comparisons: 0,
        native_driver: NATIVE_DRIVER,
    })
}
