//! Independent actual native observations; no reference/expected input is read.
use serde_json::{Value, json};
use spellwake_engine::combat::CombatRules;
use spellwake_engine::{
    draw::GameOutcome,
    effects::{EffectWindow, Program, SeedCard},
    game::{GameCommand, GameContext, GameId, GameInput, GameState},
    mana::{FloatingMana, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, GameObjects, ObjectDestination, ObjectZone},
    priority::{PriorityPhase, SeatId},
    setup::{GameSeed, StartingLife},
    stack_window::WindowPhase,
    turn::{Step, TurnStart},
};
const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);
fn name(player: SeatId) -> &'static str {
    if player == A {
        "A"
    } else if player == B {
        "B"
    } else {
        panic!("unexpected fixture participant")
    }
}
fn context(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
) -> GameContext {
    GameContext::new(
        "setup-control".into(),
        vocabulary.rules_revision().to_owned(),
        "authored".into(),
        "literal".into(),
        "mechanism-fixture".into(),
        vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn wurm(id: u64, controller: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: controller,
        destination,
        program: Program::creature(
            Some(ManaCost {
                generic: 4.into(),
                required: std::collections::BTreeMap::from([(ManaType::Green, 2.into())]),
            }),
            6.into(),
            4.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}
fn rod(id: u64, controller: SeatId) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: controller,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::artifact(None, vec![]),
    }
}
fn input(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    cards: Vec<SeedCard>,
    a: i64,
    b: i64,
) -> GameSeed {
    // Fixture premise: seeded BF timestamps follow this authored card sequence.
    let initial_battlefield_order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    GameSeed {
        name_rules: std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        creature_vocabulary: vocabulary.clone(),
        seats: vec![A, B],
        active: A,
        combat_rules: CombatRules::UnrestrictedFreeForAll,
        cards,
        initial_battlefield_order,
        initial_world_cohorts: vec![],
        mana: vec![],
        life: vec![
            StartingLife {
                player: A,
                total: a.into(),
            },
            StartingLife {
                player: B,
                total: b.into(),
            },
        ],
    }
}
fn registry_facts(cards: &GameObjects, labels: &[(u64, &str)]) -> Value {
    let mut result = serde_json::Map::new();
    for &(id, label) in labels {
        let object = cards
            .get(cards.current_identity(CardId(id)).unwrap())
            .unwrap();
        let zone = match object.zone() {
            ObjectZone::Battlefield => "Battlefield",
            ObjectZone::Hand(_) => "Hand",
            other => panic!("unexpected observed zone {other:?}"),
        };
        result.insert(label.to_owned(),json!({"owner":name(object.owner()),"controller":object.controller().map(name),"zone":zone,"continuous":object.has_continuous_control_since_turn_start()}));
    }
    json!(result)
}
fn observe(
    rows: &mut Vec<Value>,
    id: &str,
    mode: &str,
    state: &EffectWindow,
    labels: &[(u64, &str)],
    outcome: Option<GameOutcome>,
) {
    let result = match outcome {
        None => None,
        Some(GameOutcome::Winner(p)) => Some(format!("winner:{}", name(p))),
        Some(GameOutcome::Draw) => Some("draw".to_owned()),
    };
    rows.push(spellwake_reference_tools::capture_points::mark_value("setup::observe actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::EffectWindow(state),&spellwake_reference_tools::packet::AliasContext::default(),json!({"id":id,"mode":mode,"facts":{"life":{"A":state.life_total(A).unwrap(),"B":state.life_total(B).unwrap()},"cards":registry_facts(state.window().cards(),labels),"outcome":result}})).expect("collect actual setup point"));
}
fn primitive(rows: &mut Vec<Value>, id: &str, cards: &GameObjects, labels: &[(u64, &str)]) {
    rows.push(spellwake_reference_tools::capture_points::mark_value("setup::primitive actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameObjects{cards,participants:&[A,B]},&spellwake_reference_tools::packet::AliasContext::default(),
        json!({"id":id,"mode":"registry-primitive","facts":{"cards":registry_facts(cards,labels)}}),
    ).expect("collect actual setup point"));
}
fn game(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    seed: GameSeed,
    step: Step,
) -> Result<GameState, String> {
    spellwake_reference_tools::current_capture::initialize_scheduled(
        GameId(701),
        context(vocabulary),
        seed,
        TurnStart { number: 1, step },
    )
    .map(|c| c.into_state())
    .map_err(|e| format!("{e:?}"))
}
fn main() -> Result<(), String> {
    let vocabulary = spellwake_reference_tools::creature_types::selected_vocabulary()?;

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
        "setup",
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    )?;
    let mut rows = vec![];
    let labels = &[(1, "wurmA"), (2, "rodA"), (3, "wurmB"), (4, "rodB")];
    let seeded = input(
        &vocabulary,
        vec![
            wurm(1, A, ObjectDestination::Battlefield { controller: A }),
            rod(2, A),
            wurm(3, B, ObjectDestination::Battlefield { controller: B }),
            rod(4, B),
        ],
        20,
        20,
    );
    let construction = spellwake_reference_tools::native_calls::effect_initialize(seeded.clone())
        .map_err(|e| format!("{e:?}"))?;
    observe(
        &mut rows,
        "seeded-before-turn",
        "executor-construction",
        &construction,
        labels,
        None,
    );
    let own = game(&vocabulary, seeded, Step::Untap)?;
    observe(
        &mut rows,
        "seeded-A-upkeep",
        "root",
        own.execution(),
        labels,
        own.outcome(),
    );
    let cards = own.execution().window().cards();
    let next = spellwake_reference_tools::native_calls::card_turn_start(
        cards,
        cards.revision(),
        B,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?;
    primitive(&mut rows, "seeded-B-upkeep", &next, labels);

    let mut seed = input(
        &vocabulary,
        vec![wurm(1, A, ObjectDestination::Hand)],
        20,
        20,
    );
    seed.mana = vec![
        FloatingMana {
            player: A,
            mana_type: ManaType::Colourless,
            amount: 4.into(),
        },
        FloatingMana {
            player: A,
            mana_type: ManaType::Green,
            amount: 2.into(),
        },
    ];
    let mut cast = game(&vocabulary, seed, Step::PrecombatMain)?;
    let labels = &[(1, "wurmA")];
    observe(
        &mut rows,
        "cast-before",
        "root",
        cast.execution(),
        labels,
        cast.outcome(),
    );
    let spell = cast
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let candidate = spellwake_reference_tools::current_capture::advance(
        &cast,
        GameInput {
            basis: cast.basis().clone(),
            actor: A,
            command: GameCommand::CastUntargeted {
                spell,
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![
                        ManaSpend {
                            mana_type: ManaType::Colourless,
                            amount: 4.into(),
                        },
                        ManaSpend {
                            mana_type: ManaType::Green,
                            amount: 2.into(),
                        },
                    ],
                }),
            },
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut commands = vec![spellwake_reference_tools::capture_points::mark_value(
        "setup actual root cast writer",
        spellwake_reference_tools::current_profile::SnapshotOwner::GameState(candidate.state()),
        &spellwake_reference_tools::packet::AliasContext::default(),
        json!({"command":"CastUntargeted","actor":"A","payment":"4C2G"}),
    )?];
    cast = candidate.into_state();
    for _ in 0..2 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder, .. }) =
            cast.execution().window().phase()
        else {
            return Err("missing cast priority".into());
        };
        commands.push(spellwake_reference_tools::capture_points::mark_value(
            "setup actual root command writer",
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&cast),
            &spellwake_reference_tools::packet::AliasContext::default(),
            json!({"command":"Pass","actor":name(holder)}),
        )?);
        cast = spellwake_reference_tools::current_capture::advance(
            &cast,
            GameInput {
                basis: cast.basis().clone(),
                actor: holder,
                command: GameCommand::Pass,
            },
        )
        .map_err(|e| format!("{e:?}"))?
        .into_state();
    }
    observe(
        &mut rows,
        "cast-resolved",
        "root",
        cast.execution(),
        labels,
        cast.outcome(),
    );
    let cards = cast.execution().window().cards();
    let opponent = spellwake_reference_tools::native_calls::card_turn_start(
        cards,
        cards.revision(),
        B,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?;
    primitive(&mut rows, "cast-B-upkeep", &opponent, labels);
    let own = spellwake_reference_tools::native_calls::card_turn_start(
        &opponent,
        opponent.revision(),
        A,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?;
    primitive(&mut rows, "cast-next-A-upkeep", &own, labels);

    let ready = game(
        &vocabulary,
        input(
            &vocabulary,
            vec![wurm(1, A, ObjectDestination::Battlefield { controller: A })],
            20,
            20,
        ),
        Step::Untap,
    )?;
    let cards = ready.execution().window().cards();
    let id = cards.current_identity(CardId(1)).unwrap();
    primitive(&mut rows, "same-before", cards, labels);
    let same = spellwake_reference_tools::native_calls::card_controller(
        cards,
        cards.revision(),
        id,
        A,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?
    .next;
    primitive(&mut rows, "same-after", &same, labels);
    primitive(&mut rows, "change-before", cards, labels);
    let changed = spellwake_reference_tools::native_calls::card_controller(
        cards,
        cards.revision(),
        id,
        B,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?
    .next;
    primitive(&mut rows, "change-to-B", &changed, labels);
    let b = spellwake_reference_tools::native_calls::card_turn_start(
        &changed,
        changed.revision(),
        B,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?;
    primitive(&mut rows, "change-B-upkeep", &b, labels);
    primitive(&mut rows, "away-before", cards, labels);
    let away = spellwake_reference_tools::native_calls::card_controller(
        cards,
        cards.revision(),
        id,
        B,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?
    .next;
    primitive(&mut rows, "away-to-B", &away, labels);
    let back = spellwake_reference_tools::native_calls::card_controller(
        &away,
        away.revision(),
        id,
        A,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?
    .next;
    primitive(&mut rows, "away-back-to-A", &back, labels);
    let b = spellwake_reference_tools::native_calls::card_turn_start(
        &back,
        back.revision(),
        B,
        &[A, B],
    )
    .map_err(|e| format!("{e:?}"))?;
    primitive(&mut rows, "away-B-upkeep", &b, labels);
    let a = spellwake_reference_tools::native_calls::card_turn_start(&b, b.revision(), A, &[A, B])
        .map_err(|e| format!("{e:?}"))?;
    primitive(&mut rows, "away-next-A-upkeep", &a, labels);

    for (label, a, b) in [("forty", 40, 40), ("nonuniform", 20, 40)] {
        let raw = spellwake_reference_tools::native_calls::effect_initialize(input(
            &vocabulary,
            vec![],
            a,
            b,
        ))
        .map_err(|e| format!("{e:?}"))?;
        observe(
            &mut rows,
            &format!("{label}-before"),
            "executor-construction",
            &raw,
            &[],
            None,
        );
        let root = game(&vocabulary, input(&vocabulary, vec![], a, b), Step::Untap)?;
        observe(
            &mut rows,
            &format!("{label}-A-upkeep"),
            "root",
            root.execution(),
            &[],
            root.outcome(),
        );
    }
    for (label, a, b) in [("zero", 0, 20), ("negative", -1, 40), ("both-zero", 0, 0)] {
        let raw = spellwake_reference_tools::native_calls::effect_initialize(input(
            &vocabulary,
            vec![],
            a,
            b,
        ))
        .map_err(|e| format!("{e:?}"))?;
        assert!(raw.state_based_losses().is_empty());
        observe(
            &mut rows,
            &format!("{label}-before"),
            "executor-construction",
            &raw,
            &[],
            None,
        );
        let root = game(&vocabulary, input(&vocabulary, vec![], a, b), Step::Untap)?;
        observe(
            &mut rows,
            &format!("{label}-ended"),
            "root",
            root.execution(),
            &[],
            root.outcome(),
        );
    }
    let mut observations = serde_json::json!({"schema_version":1,"profile":"setup-control","observations":rows,"root_cast_commands":commands,"limits":["Registry-only rows compare card facts only; no native upkeep advancement or life/outcome claimed","Prepared-main cast is actual native admission/resolution, without invented prior turn history","Native root still stops at combat; three after-untap intermediate rows are reference-only","No qualified complete cards/games or client-safe projection"]});
    spellwake_reference_tools::capture_points::bind_value_points(
        &mut observations,
        "/observations",
    )?;
    let envelope = spellwake_reference_tools::current_capture::finish("setup", observations)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).expect("serialize actual current capture")
    );
    Ok(())
}
