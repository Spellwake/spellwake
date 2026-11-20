//! Actual native action trials and candidates; no expected/reference artifact input.
use serde::Serialize;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    effects::{Instruction, Program, SeedCard},
    game::{GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate},
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaSpend,
        ManaType,
    },
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::collections::BTreeMap;
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn alias(s: SeatId) -> &'static str {
    match s {
        A => "A",
        B => "B",
        _ => panic!("unknown fixture seat"),
    }
}
fn seed(card: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination,
        program,
    }
}
fn land() -> Program {
    Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(0),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 1.into())])),
        }],
    )
}
fn sorcery() -> Program {
    Program::sorcery(
        ManaCost {
            generic: 1.into(),
            required: BTreeMap::from([(ManaType::Red, 1.into())]),
        },
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 3.into() }],
        ),
    )
}
fn instant() -> Program {
    Program::instant(
        ManaCost::one(ManaType::Red),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 2.into() }],
        ),
    )
}
fn fixture(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    step: Step,
    spells: bool,
) -> GameState {
    let mut cards = vec![
        seed(1, A, ObjectDestination::Hand, land()),
        seed(11, A, ObjectDestination::Hand, land()),
        seed(12, B, ObjectDestination::Hand, land()),
    ];
    if spells {
        cards.extend([
            seed(2, A, ObjectDestination::Hand, sorcery()),
            seed(3, B, ObjectDestination::Hand, instant()),
            seed(5, A, ObjectDestination::Hand, sorcery()),
            seed(6, A, ObjectDestination::Hand, instant()),
            seed(7, B, ObjectDestination::Hand, sorcery()),
            seed(
                4,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::creature(None, 6.into(), 4.into(), std::collections::BTreeSet::new()),
            ),
        ]);
    }
    for i in 20..25 {
        cards.push(seed(i, B, ObjectDestination::LibraryBottom, land()));
    }
    spellwake_reference_tools::current_capture::initialize_scheduled(
        GameId(9),
        GameContext::new(
            "native-land-v1".into(),
            vocabulary.rules_revision().to_owned(),
            "authored".into(),
            "land-timing".into(),
            "individual".into(),
            vocabulary.id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
        {
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
                vec![
                    FloatingMana {
                        player: A,
                        mana_type: ManaType::Red,
                        amount: 8.into(),
                    },
                    FloatingMana {
                        player: B,
                        mana_type: ManaType::Red,
                        amount: 8.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart { number: 1, step },
    )
    .unwrap()
    .into_state()
}
fn object(s: &GameState, id: u64) -> ObjectId {
    s.execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn go(
    s: &GameState,
    actor: SeatId,
    command: GameCommand,
) -> Result<NativeCandidate, spellwake_engine::game::GameError> {
    spellwake_reference_tools::current_capture::advance(
        s,
        GameInput {
            basis: s.basis().clone(),
            actor,
            command,
        },
    )
}
fn pass(s: GameState, actor: SeatId) -> GameState {
    go(&s, actor, GameCommand::Pass).unwrap().into_state()
}
fn play(s: &GameState, id: u64) -> GameCommand {
    GameCommand::PlayLand {
        land: object(s, id),
    }
}
fn cast(s: &GameState, id: u64) -> GameCommand {
    let target = TargetRef::Object(object(s, 4));
    if [2, 5, 7].contains(&id) {
        GameCommand::CastPaid {
            spell: object(s, id),
            target,
            payment: ManaPayment {
                spend: vec![ManaSpend {
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                }],
            },
        }
    } else {
        GameCommand::Cast {
            spell: object(s, id),
            target,
        }
    }
}
fn zone(s: &GameState, id: u64) -> &'static str {
    match s
        .execution()
        .window()
        .cards()
        .get(object(s, id))
        .unwrap()
        .zone()
    {
        ObjectZone::Hand(_) => "Hand",
        ObjectZone::Battlefield => "Battlefield",
        ObjectZone::Stack => "Stack",
        ObjectZone::Graveyard(_) => "Graveyard",
        _ => panic!("unexpected fixture zone"),
    }
}
fn holder(s: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
    else {
        panic!("not ready")
    };
    holder
}
fn step(s: &GameState) -> &'static str {
    match s.turn().unwrap().position().step {
        Step::Upkeep => "UPKEEP",
        Step::PrecombatMain => "MAIN1",
        Step::PostcombatMain => "MAIN2",
        Step::BeginningCombat => "COMBAT_BEGIN",
        _ => panic!("unobserved fixture step"),
    }
}
#[derive(Serialize)]
struct Row {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,
    id: &'static str,
    kind: &'static str,
    allowed: Option<bool>,
    turn: u64,
    step: &'static str,
    active: &'static str,
    actor: &'static str,
    priority: &'static str,
    stack_size: usize,
    land_zone: &'static str,
    lands_played: i64,
    land_tapped: Option<bool>,
    spell_zone: Option<&'static str>,
    response_zone: Option<&'static str>,
    target_zone: Option<&'static str>,
    target_damage: Option<i64>,
    protocol: &'static str,
}
fn observe(
    s: &GameState,
    id: &'static str,
    kind: &'static str,
    actor: SeatId,
    land_id: u64,
    allowed: Option<bool>,
    protocol: &'static str,
) -> Row {
    let observed_zone = |id| {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(id))
            .map(|_| zone(s, id))
    };
    let target = s
        .execution()
        .window()
        .cards()
        .current_identity(CardId(4))
        .and_then(|id| {
            s.execution()
                .creature(id)
                .expect("authored fixture characteristic invariants")
        });
    Row {
        capture_point: spellwake_reference_tools::capture_points::token(
            "observe actual scoped row",
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(s),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("collect actual scoped row"),
        id,
        kind,
        allowed,
        turn: s.turn().unwrap().position().number,
        step: step(s),
        active: alias(s.execution().window().turn_owner()),
        actor: alias(actor),
        priority: alias(holder(s)),
        stack_size: s.execution().window().entries().len(),
        land_zone: zone(s, land_id),
        lands_played: if actor == s.execution().window().turn_owner() {
            String::from(s.turn().unwrap().lands_played().clone())
                .parse()
                .unwrap()
        } else {
            0
        },
        land_tapped: s
            .execution()
            .window()
            .cards()
            .get(object(s, land_id))
            .unwrap()
            .tapped(),
        spell_zone: observed_zone(2),
        response_zone: observed_zone(3),
        target_zone: observed_zone(4),
        target_damage: target.map(|v| String::from(v.damage).parse().unwrap()),
        protocol,
    }
}
fn probe(
    rows: &mut Vec<Row>,
    s: &GameState,
    id: &'static str,
    kind: &'static str,
    actor: SeatId,
    land_id: u64,
    command: GameCommand,
) {
    let before = s.clone();
    let allowed = spellwake_reference_tools::current_capture::advance_with_adoption(
        s,
        GameInput {
            basis: s.basis().clone(),
            actor,
            command,
        },
        false,
    )
    .is_ok();
    assert_eq!(s, &before);
    rows.push(observe(
        s,
        id,
        kind,
        actor,
        land_id,
        Some(allowed),
        "actual-native-candidate-trial-discarded",
    ));
}
fn land_main1(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Vec<Row> {
    let mut rows = vec![];
    let mut s = fixture(vocabulary, Step::Upkeep, false);
    probe(
        &mut rows,
        &s,
        "L1-upkeep-A",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    s = pass(pass(s, A), B);
    probe(
        &mut rows,
        &s,
        "L1-main1-A-before",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    let played = go(&s, A, play(&s, 1)).unwrap();
    assert!(played.land_play().is_some());
    s = played.into_state();
    rows.push(observe(
        &s,
        "L1-after-first",
        "land-play",
        A,
        1,
        Some(true),
        "actual-native-action",
    ));
    probe(
        &mut rows,
        &s,
        "L1-main1-A-second",
        "land-permission",
        A,
        11,
        play(&s, 11),
    );
    s = pass(s, A);
    probe(
        &mut rows,
        &s,
        "L1-main1-B-opponent",
        "land-permission",
        B,
        12,
        play(&s, 12),
    );
    rows
}
fn land_main2(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Vec<Row> {
    let mut rows = vec![];
    let mut s = fixture(vocabulary, Step::Upkeep, false);
    probe(
        &mut rows,
        &s,
        "L2-upkeep-A",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    s = pass(pass(s, A), B);
    probe(
        &mut rows,
        &s,
        "L2-main1-before",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    s = pass(s, A);
    probe(
        &mut rows,
        &s,
        "L2-main1-B-opponent",
        "land-permission",
        B,
        12,
        play(&s, 12),
    );
    s = pass(s, B);
    probe(
        &mut rows,
        &s,
        "L2-combat-A",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    // Native combat remains blocked; this is an independent prepared MAIN2 fixture.
    s = fixture(vocabulary, Step::PostcombatMain, false);
    probe(
        &mut rows,
        &s,
        "L2-main2-before",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    s = go(&s, A, play(&s, 1)).unwrap().into_state();
    rows.push(observe(
        &s,
        "L2-main2-after",
        "land-play",
        A,
        1,
        Some(true),
        "actual-native-prepared-main2-action",
    ));
    probe(
        &mut rows,
        &s,
        "L2-main2-second",
        "land-permission",
        A,
        11,
        play(&s, 11),
    );
    rows
}
fn timing(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> Vec<Row> {
    let mut rows = vec![];
    let mut s = fixture(vocabulary, Step::Upkeep, true);
    for (id, kind, card) in [
        ("T-upkeep-A-instant", "instant-permission", 6),
        ("T-upkeep-A-sorcery", "sorcery-permission", 2),
    ] {
        probe(&mut rows, &s, id, kind, A, 1, cast(&s, card));
    }
    s = pass(s, A);
    for (id, kind, card) in [
        ("T-upkeep-B-instant", "instant-permission", 3),
        ("T-upkeep-B-sorcery", "sorcery-permission", 7),
    ] {
        probe(&mut rows, &s, id, kind, B, 12, cast(&s, card));
    }
    // Independent prepared MAIN1 restores disclosed floating-mana fixtures, not a
    // production ability or an assertion that upkeep mana survives its boundary.
    s = fixture(vocabulary, Step::PrecombatMain, true);
    for (id, kind, card) in [
        ("T-main1-A-instant", "instant-permission", 6),
        ("T-main1-A-sorcery", "sorcery-permission", 2),
    ] {
        probe(&mut rows, &s, id, kind, A, 1, cast(&s, card));
    }
    probe(
        &mut rows,
        &s,
        "T-main1-A-land",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    s = go(&s, A, cast(&s, 2)).unwrap().into_state();
    rows.push(observe(
        &s,
        "T-after-sorcery-cast",
        "sorcery-cast",
        A,
        1,
        Some(true),
        "actual-native-action",
    ));
    for (id, kind, card) in [
        ("T-occupied-A-instant", "instant-permission", 6),
        ("T-occupied-A-sorcery", "sorcery-permission", 5),
    ] {
        probe(&mut rows, &s, id, kind, A, 1, cast(&s, card));
    }
    probe(
        &mut rows,
        &s,
        "T-occupied-A-land",
        "land-permission",
        A,
        1,
        play(&s, 1),
    );
    s = pass(s, A);
    for (id, kind, card) in [
        ("T-occupied-B-instant", "instant-permission", 3),
        ("T-occupied-B-sorcery", "sorcery-permission", 7),
    ] {
        probe(&mut rows, &s, id, kind, B, 12, cast(&s, card));
    }
    s = go(&s, B, cast(&s, 3)).unwrap().into_state();
    rows.push(observe(
        &s,
        "T-after-instant-response",
        "instant-cast",
        B,
        12,
        Some(true),
        "actual-native-action",
    ));
    s = pass(pass(s, B), A);
    rows.push(observe(
        &s,
        "T-after-response-resolution",
        "resolved-boundary",
        A,
        1,
        None,
        "actual-native-resolution-and-checks",
    ));
    s = pass(pass(s, A), B);
    rows.push(observe(
        &s,
        "T-after-sorcery-resolution",
        "resolved-boundary",
        A,
        1,
        None,
        "actual-native-resolution-and-checks",
    ));
    rows
}
fn main() {
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

    spellwake_reference_tools::current_capture::start(
        "land",
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    )
    .unwrap_or_else(|error| {
        eprintln!("capture setup failed: {error}");
        std::process::exit(1)
    });
    let scenarios = BTreeMap::from([
        ("land-main1", land_main1(&vocabulary)),
        ("land-main2", land_main2(&vocabulary)),
        ("timing-response", timing(&vocabulary)),
    ]);
    let mut observations = serde_json::json!({"schema_version":1,"scope":"Authored ordinary land/default timing through actual GameState; prepared phase boundaries explicitly independent, no complete combat/turn qualification", "scenarios":scenarios,"limits":["Candidate trials execute and discard actual successors, unlike reference canPlay queries; common permission points have sufficient resources and supported legal targets.","MAIN2 and funded timing MAIN1 are separate prepared fixtures, not traversal through unsupported combat or preservation of upkeep mana.","L1 MAIN2/turn2/turn3 and T MAIN2 remain reference-only; native reset/cross-main continuity are independently tested with explicit root/scheduler boundaries.","No complete card, dynamic permission/land allowance, ETB, public projection, replay or performance qualification."]});
    spellwake_reference_tools::capture_points::bind_value_points(
        &mut observations,
        "/observations",
    )
    .unwrap_or_else(|error| {
        eprintln!("capture output failed: {error}");
        std::process::exit(1)
    });
    let envelope = spellwake_reference_tools::current_capture::finish("land", observations)
        .unwrap_or_else(|error| {
            eprintln!("capture output failed: {error}");
            std::process::exit(1)
        });
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).expect("serialize actual current capture")
    );
}
