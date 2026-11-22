//! Actual catalogue-backed teaching games and ordinary-path regressions.
use spellwake_reference_tools::ordinary_game::{
    self,
    fixture::CaseId,
    teaching::{TeachingCase, TeachingSource},
};
use std::path::PathBuf;
fn source(
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> TeachingSource {
    let dir = PathBuf::from(
        std::env::var_os("SPELLWAKE_TEACHING_CATALOGUE_DIR")
            .expect("explicit actual pinned metadata/gzip directory required"),
    );
    TeachingSource::load(
        &dir.join("oracle-metadata.json"),
        &dir.join("oracle-cards-20261029210158.jsonl.gz"),
        vocabulary,
    )
    .expect("actual pinned teaching source required")
}
#[test]
#[ignore = "requires actual pinned catalogue metadata/gzip in explicit SPELLWAKE_TEACHING_CATALOGUE_DIR plus source-qualified vocabulary; execute separately"]
fn catalogue_teaching_trajectories_reach_natural_outcomes_and_reexecute() {
    let vocabulary = spellwake_reference_tools::creature_types::selected_vocabulary()
        .expect("explicit source-qualified vocabulary");
    let source = source(&vocabulary);
    let report = source.run(&TeachingCase::ALL, vocabulary).unwrap();
    let json = serde_json::to_value(&report).unwrap();
    for run in json["runs"].as_array().unwrap() {
        assert_eq!(
            run["completed"], true,
            "{} failure {}",
            run["fixture_ref"], run["failure"]
        );
        assert_eq!(run["rerun"]["final_state_equal"], true);
        assert_eq!(run["rerun"]["all_inputs_consumed"], true);
    }
    assert_teaching_semantics(&json);
    assert!(report.completed());
}
#[test]
fn all_four_predecessor_trajectories_keep_their_actual_reexecution() {
    let vocabulary = spellwake_reference_tools::creature_types::selected_vocabulary()
        .expect("explicit source-qualified vocabulary");
    let report = ordinary_game::run_cases(&CaseId::ALL, vocabulary);
    let json = serde_json::to_value(&report).unwrap();
    for run in json["runs"].as_array().unwrap() {
        assert_eq!(
            run["completed"], true,
            "{} failure {}",
            run["fixture_ref"], run["failure"]
        );
        assert_eq!(run["rerun"]["final_state_equal"], true);
    }
    assert!(report.completed());
}

use spellwake_engine::{
    actions::TimingError,
    characteristics::Color,
    effects::EffectError,
    game::{DecisionReply, GameCommand, GameError, GameInput, GameState, RuleFrame},
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectId, ObjectZone},
    opening::{MulliganChoice, OpeningDecision},
    priority::PriorityPhase,
    program::{CardType, ProgramKind, Supertype},
    stack_window::WindowPhase,
    turn::{Step, TurnError},
};
use std::collections::{BTreeMap, BTreeSet};
fn object(state: &GameState, card: CardId) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(card)
        .unwrap()
}
fn rejected(state: &GameState, input: GameInput, expected: GameError) {
    let retained = state.clone();
    assert_eq!(state.advance(input).unwrap_err(), expected);
    assert_eq!(state, &retained);
}
#[test]
#[ignore = "requires actual pinned catalogue metadata/gzip in explicit SPELLWAKE_TEACHING_CATALOGUE_DIR plus source-qualified vocabulary; execute separately"]
fn all_twenty_selected_identities_follow_guarded_ordinary_lifecycles() {
    let vocabulary = spellwake_reference_tools::creature_types::selected_vocabulary()
        .expect("explicit source-qualified vocabulary");
    let source = source(&vocabulary);
    let cases = source.lifecycle_inputs(vocabulary.clone()).unwrap();
    assert_eq!(cases.len(), 20);
    let mut ids = BTreeSet::new();
    for case in cases {
        assert!(ids.insert(case.id.clone()));
        let actual = source
            .inventory()
            .cards
            .iter()
            .find(|c| c.oracle_id.as_deref() == Some(&case.oracle_id))
            .unwrap();
        let face = &actual.faces[0];
        let land = case.program.has_type(CardType::Land);
        let color = if case.owner.0 == 41 {
            ManaType::White
        } else {
            ManaType::Green
        };
        let basic = if case.owner.0 == 41 {
            BasicLandType::Plains
        } else {
            BasicLandType::Forest
        };
        let cost = match &case.program.kind {
            ProgramKind::Permanent(def) => def.casting_cost.as_ref().map(|c| {
                serde_json::to_value(c.total())
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()
            }),
            _ => None,
        };
        let turn_to_cast = cost.unwrap_or(1) as u64;
        let lands = case
            .setup
            .seed
            .seats
            .iter()
            .map(|&owner| {
                (
                    owner,
                    case.setup
                        .seed
                        .cards
                        .iter()
                        .filter(|c| c.owner == owner && c.program.has_type(CardType::Land))
                        .map(|c| c.card)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let initial =
            GameState::initialize_opening(case.game, case.context.clone(), case.setup.clone())
                .unwrap();
        let mut state = initial.into_state();
        assert_eq!(state.execution().window().cards().iter().count(), 120);
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(object(&state, case.target))
                .unwrap()
                .zone(),
            ObjectZone::Hand(case.owner)
        );
        let mut played = BTreeSet::new();
        let mut observed_stack = false;
        let mut negatives = false;
        let mut insufficient = false;
        let mut steps = 0;
        let mut finished = false;
        while steps < 512 {
            steps += 1;
            let target = object(&state, case.target);
            let target_object = state.execution().window().cards().get(target).unwrap();
            if target_object.zone() == ObjectZone::Battlefield {
                let chars = state
                    .execution()
                    .current_characteristics(target)
                    .unwrap()
                    .unwrap();
                assert_eq!(chars.current_names, case.program.names, "{} name", case.id);
                assert_eq!(
                    chars.current_types, case.program.card_types,
                    "{} types",
                    case.id
                );
                assert_eq!(
                    chars.current_supertypes, case.program.supertypes,
                    "{} supertypes",
                    case.id
                );
                assert_eq!(
                    chars.current_creature_subtypes, case.program.creature_subtypes,
                    "{} creature subtypes",
                    case.id
                );
                assert_eq!(
                    chars.current_land_subtypes, case.program.land_subtypes,
                    "{} land subtypes",
                    case.id
                );
                if land {
                    assert_eq!(chars.intrinsic_mana, BTreeSet::from([basic]));
                    assert!(chars.current_supertypes.contains(&Supertype::Basic));
                    assert!(chars.current_colors.is_empty());
                    assert!(state.execution().window().entries().is_empty());
                    let input = GameInput {
                        basis: state.basis().clone(),
                        actor: case.owner,
                        command: GameCommand::ActivateMana {
                            source: target,
                            ability: ManaAbilityRef::BasicLand(basic),
                        },
                    };
                    let next = state.advance(input).unwrap();
                    assert_eq!(next.state().execution().mana(case.owner, color), 1.into());
                    assert_eq!(
                        next.state()
                            .execution()
                            .window()
                            .cards()
                            .get(target)
                            .unwrap()
                            .tapped(),
                        Some(true)
                    );
                } else {
                    assert!(observed_stack, "{} actual cast stack entry", case.id);
                    assert!(
                        insufficient,
                        "{} actual insufficient-payment rejection",
                        case.id
                    );
                    assert_eq!(
                        chars.current_colors,
                        BTreeSet::from([if case.owner.0 == 41 {
                            Color::White
                        } else {
                            Color::Green
                        }])
                    );
                    let pt = chars.power_toughness.unwrap().derived;
                    assert_eq!(
                        serde_json::to_value(pt.power).unwrap().as_str().unwrap(),
                        face.power.as_deref().unwrap()
                    );
                    assert_eq!(
                        serde_json::to_value(pt.toughness)
                            .unwrap()
                            .as_str()
                            .unwrap(),
                        face.toughness.as_deref().unwrap()
                    );
                    assert_eq!(
                        target_object.has_continuous_control_since_turn_start(),
                        Some(false)
                    );
                    assert!(
                        case.program.triggered.is_empty()
                            && case.program.activated.is_empty()
                            && case.program.mana.is_empty()
                            && case.program.continuous.is_none()
                    );
                }
                finished = true;
                break;
            }
            let (actor, command) = if let Some(pending) = state.pending_decision() {
                let choice = match pending.frame() {
                    RuleFrame::Opening(OpeningDecision::Declare { .. }) => {
                        DecisionReply::Mulligan(MulliganChoice::Keep)
                    }
                    _ => panic!(
                        "{} unexpected lifecycle frame {:?}",
                        case.id,
                        pending.frame()
                    ),
                };
                (
                    pending.chooser(),
                    GameCommand::Reply {
                        decision: pending.id(),
                        choice,
                    },
                )
            } else {
                let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                    state.execution().window().phase()
                else {
                    panic!("{} no priority/decision", case.id)
                };
                let position = state.turn().unwrap().position();
                let own = position.number.div_ceil(2);
                let active = state.execution().window().active_player();
                if !negatives && holder == case.owner && position.step == Step::Upkeep {
                    let input = GameInput {
                        basis: state.basis().clone(),
                        actor: holder,
                        command: GameCommand::Pass,
                    };
                    let mut stale = input.clone();
                    stale.basis.revision -= 1;
                    rejected(&state, stale, GameError::StaleRevision);
                    let mut foreign = input;
                    foreign.actor = spellwake_engine::priority::SeatId(999);
                    rejected(&state, foreign, GameError::UnknownActor);
                    if !land {
                        let payment = ManaPayment {
                            from_lots: Default::default(),
                            spend: vec![ManaSpend {
                                mana_type: color,
                                amount: spellwake_contracts::RulesInteger::from_usize(
                                    cost.unwrap(),
                                ),
                            }],
                        };
                        rejected(
                            &state,
                            GameInput {
                                basis: state.basis().clone(),
                                actor: holder,
                                command: GameCommand::CastUntargeted {
                                    spell: target,
                                    payment: Some(payment),
                                },
                            },
                            GameError::Effect(EffectError::Timing(TimingError::MainPhaseRequired)),
                        );
                    }
                    negatives = true;
                }
                if holder == active
                    && position.step == Step::PrecombatMain
                    && state.execution().window().entries().is_empty()
                {
                    if !played.contains(&(active, own)) {
                        let card = lands[&active][own as usize - 1];
                        played.insert((active, own));
                        (
                            holder,
                            GameCommand::PlayLand {
                                land: object(&state, card),
                            },
                        )
                    } else if active == case.owner && own == turn_to_cast && !land {
                        let amount = state.execution().mana(case.owner, color);
                        if !insufficient {
                            let input = GameInput {
                                basis: state.basis().clone(),
                                actor: holder,
                                command: GameCommand::CastUntargeted {
                                    spell: target,
                                    payment: Some(ManaPayment {
                                        from_lots: Default::default(),
                                        spend: vec![ManaSpend {
                                            mana_type: color,
                                            amount: spellwake_contracts::RulesInteger::from_usize(
                                                cost.unwrap(),
                                            ),
                                        }],
                                    }),
                                },
                            };
                            rejected(
                                &state,
                                input,
                                GameError::Effect(EffectError::InsufficientMana),
                            );
                            insufficient = true;
                        }
                        if amount < spellwake_contracts::RulesInteger::from_usize(cost.unwrap()) {
                            let card = *lands[&active]
                                .iter()
                                .find(|&&c| {
                                    state
                                        .execution()
                                        .window()
                                        .cards()
                                        .get(object(&state, c))
                                        .is_some_and(|o| {
                                            o.zone() == ObjectZone::Battlefield
                                                && o.tapped() == Some(false)
                                        })
                                })
                                .unwrap();
                            (
                                holder,
                                GameCommand::ActivateMana {
                                    source: object(&state, card),
                                    ability: ManaAbilityRef::BasicLand(basic),
                                },
                            )
                        } else {
                            (
                                holder,
                                GameCommand::CastUntargeted {
                                    spell: target,
                                    payment: Some(ManaPayment {
                                        from_lots: Default::default(),
                                        spend: vec![ManaSpend {
                                            mana_type: color,
                                            amount: spellwake_contracts::RulesInteger::from_usize(
                                                cost.unwrap(),
                                            ),
                                        }],
                                    }),
                                },
                            )
                        }
                    } else {
                        (holder, GameCommand::Pass)
                    }
                } else {
                    (holder, GameCommand::Pass)
                }
            };
            let input = GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            };
            let candidate = state
                .advance(input.clone())
                .unwrap_or_else(|e| panic!("{} step{steps} {:?}: {e:?}", case.id, input.command));
            if matches!(input.command, GameCommand::PlayLand { .. }) && actor == case.owner {
                let next = candidate.state();
                let second = object(
                    next,
                    lands[&actor][state.turn().unwrap().position().number.div_ceil(2) as usize],
                );
                rejected(
                    next,
                    GameInput {
                        basis: next.basis().clone(),
                        actor,
                        command: GameCommand::PlayLand { land: second },
                    },
                    GameError::Turn(TurnError::LandLimitReached),
                );
            }
            if matches!(input.command, GameCommand::CastUntargeted { .. }) {
                assert_eq!(candidate.state().execution().window().entries().len(), 1);
                assert!(matches!(
                    candidate
                        .state()
                        .execution()
                        .window()
                        .cards()
                        .get(object(candidate.state(), case.target))
                        .unwrap()
                        .zone(),
                    ObjectZone::Stack
                ));
                observed_stack = true;
            }
            state = candidate.into_state();
        }
        assert!(finished, "{} lifecycle watchdog", case.id);
        assert!(negatives, "{} reached stale/foreign rejection", case.id);
    }
    assert_eq!(ids.len(), 20);
}

fn accepted_at<'a>(run: &'a serde_json::Value, turn: u64, step: &str) -> &'a serde_json::Value {
    run["accepted"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| {
            a["header"]["turn"]["position"]["number"] == turn
                && a["header"]["turn"]["position"]["step"] == step
        })
        .unwrap_or_else(|| panic!("{} missing {turn}/{step}", run["fixture_ref"]))
}
fn actual_checkpoint<'a>(
    run: &'a serde_json::Value,
    accepted: &serde_json::Value,
) -> &'a serde_json::Value {
    &run["checkpoints"][accepted["ordinal"].as_u64().unwrap().to_string()]
}
fn checkpoint_card<'a>(checkpoint: &'a serde_json::Value, alias: &str) -> &'a serde_json::Value {
    checkpoint["cards"]
        .as_object()
        .unwrap()
        .values()
        .find(|v| v["alias"] == alias)
        .unwrap()
}
fn membership(checkpoint: &serde_json::Value, owner: u32, zone: &str) -> BTreeSet<String> {
    checkpoint["cards"]
        .as_object()
        .unwrap()
        .values()
        .filter(|c| c["object"]["owner"] == owner && c["object"]["zone"]["kind"] == zone)
        .map(|c| c["alias"].as_str().unwrap().to_owned())
        .collect()
}
fn ordered(checkpoint: &serde_json::Value, owner: u32, zone: &str) -> Vec<String> {
    checkpoint["ordered_piles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pile"]["kind"] == zone && p["pile"]["owner"] == owner)
        .unwrap()["objects_top_to_bottom"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["alias"].as_str().unwrap().to_owned())
        .collect()
}
fn names(aliases: &[&str]) -> BTreeSet<String> {
    aliases.iter().map(|v| (*v).to_owned()).collect()
}
fn assert_teaching_semantics(report: &serde_json::Value) {
    let runs = report["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    for run in runs {
        let case = run["fixture_ref"].as_str().unwrap();
        let opening = run["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "opening.complete")
            .unwrap()["candidate_ordinal"]
            .as_u64()
            .unwrap();
        let header = &run["accepted"][opening as usize - 1]["header"];
        assert_eq!(header["participants"], serde_json::json!([41, 3]));
        assert_eq!(header["counts_by_owner"]["41"]["hand"], 7);
        assert_eq!(
            header["counts_by_owner"]["3"]["hand"],
            if case == "RD01" { 7 } else { 6 }
        );
        assert_eq!(
            header["counts_by_owner"]["3"]["library"],
            if case == "RD01" { 53 } else { 54 }
        );
        assert_eq!(header["active"], if case == "RD01" { 41 } else { 3 });
        let terminal = run["accepted"].as_array().unwrap().last().unwrap();
        let h = &terminal["header"];
        assert_eq!(h["turn"]["position"]["number"], 13);
        assert_eq!(h["turn"]["position"]["step"], "combat_damage");
        assert!(h["pending"].is_null());
        assert!(h["priority_holder"].is_null());
        assert_eq!(run["final"]["entropy_remaining"], 0);
        let last = actual_checkpoint(run, terminal);
        assert_eq!(last["cards"].as_object().unwrap().len(), 120);
        for owner in [41, 3] {
            assert_eq!(
                [
                    "library",
                    "hand",
                    "battlefield",
                    "graveyard",
                    "exile",
                    "command",
                    "stack"
                ]
                .into_iter()
                .map(|z| h["counts_by_owner"][owner.to_string()][z].as_u64().unwrap())
                .sum::<u64>(),
                60
            );
        }
        let probes = run["rejection_probes"].as_array().unwrap();
        for required in ["stale", "foreign", "insufficient-payment", "second-land"] {
            assert!(
                probes.iter().any(|p| p["expectation_ref"] == required),
                "{case} missing {required}"
            );
        }
        assert!(
            probes
                .iter()
                .all(|p| p["state_retained"] == true && p["no_candidate"] == true)
        );
        if case == "RD01" {
            assert_eq!(h["life"], serde_json::json!({"41":"20","3":"0"}));
            assert_eq!(
                run["final"]["outcome"],
                serde_json::json!({"kind":"winner","seat":41})
            );
            for (turn, life) in [(3, "18"), (5, "16"), (7, "12"), (9, "8"), (11, "4")] {
                assert_eq!(
                    accepted_at(run, turn, "postcombat_main")["header"]["life"]["3"],
                    life
                );
            }
            let elite = run["accepted"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| {
                    a["input"]["command"]["kind"] == "cast_untargeted"
                        && a["input"]["command"]["spell"]["alias"] == "W.elite-vanguard.1"
                })
                .unwrap();
            assert_eq!(elite["header"]["mana"]["41"]["white"], "1");
            assert_eq!(
                accepted_at(run, 3, "beginning_combat")["header"]["mana"]["41"]["white"],
                "0"
            );
            let trade = actual_checkpoint(run, accepted_at(run, 5, "postcombat_main"));
            assert_eq!(ordered(trade, 41, "graveyard"), vec!["W.savannah-lions.1"]);
            assert_eq!(ordered(trade, 3, "graveyard"), vec!["G.grizzly-bears.1"]);
            assert_eq!(
                checkpoint_card(trade, "W.elite-vanguard.1")["object"]["zone"]["kind"],
                "battlefield"
            );
            assert_eq!(
                checkpoint_card(trade, "W.glory-seeker.1")["object"]["zone"]["kind"],
                "battlefield"
            );
            assert_eq!(h["counts_by_owner"]["41"]["library"], 47);
            assert_eq!(h["counts_by_owner"]["3"]["library"], 47);
            assert_eq!(
                membership(last, 41, "hand"),
                names(&["W.yoked-ox.1", "W.pillarfield-ox.1", "W.plains.8"])
            );
            assert_eq!(
                membership(last, 3, "hand"),
                names(&[
                    "G.forest.7",
                    "G.forest.8",
                    "G.forest.9",
                    "G.runeclaw-bear.1",
                    "G.terrain-elemental.1",
                    "G.kalonian-tusker.1"
                ])
            );
            let mut wfield = names(&["W.elite-vanguard.1", "W.glory-seeker.1"]);
            wfield.extend((1..=7).map(|n| format!("W.plains.{n}")));
            assert_eq!(membership(last, 41, "battlefield"), wfield);
            assert_eq!(
                membership(last, 3, "battlefield"),
                (1..=6).map(|n| format!("G.forest.{n}")).collect()
            );
            assert!(
                probes
                    .iter()
                    .any(|p| p["expectation_ref"] == "young-attacker")
            );
        } else {
            assert_eq!(case, "RD02");
            assert_eq!(h["life"], serde_json::json!({"41":"-11","3":"20"}));
            assert_eq!(
                run["final"]["outcome"],
                serde_json::json!({"kind":"winner","seat":3})
            );
            for (turn, life) in [(5, "20"), (7, "20"), (9, "13"), (11, "1")] {
                assert_eq!(
                    accepted_at(run, turn, "postcombat_main")["header"]["life"]["41"],
                    life
                );
            }
            let g3 = actual_checkpoint(run, accepted_at(run, 5, "postcombat_main"));
            assert_eq!(
                checkpoint_card(g3, "W.yoked-ox.1")["creature"]["marked_damage"],
                "3"
            );
            assert!(ordered(g3, 41, "graveyard").is_empty());
            let w3 = actual_checkpoint(run, accepted_at(run, 6, "upkeep"));
            assert_eq!(
                checkpoint_card(w3, "W.yoked-ox.1")["creature"]["marked_damage"],
                "0"
            );
            let g4 = actual_checkpoint(run, accepted_at(run, 7, "postcombat_main"));
            assert_eq!(
                ordered(g4, 41, "graveyard"),
                vec!["W.yoked-ox.1", "W.elite-vanguard.1"]
            );
            assert_eq!(
                checkpoint_card(g4, "G.kalonian-tusker.1")["creature"]["marked_damage"],
                "2"
            );
            let w4 = actual_checkpoint(run, accepted_at(run, 8, "upkeep"));
            assert_eq!(
                checkpoint_card(w4, "G.kalonian-tusker.1")["creature"]["marked_damage"],
                "0"
            );
            assert_eq!(
                ordered(last, 41, "graveyard"),
                vec![
                    "W.plains.4",
                    "W.plains.3",
                    "W.yoked-ox.1",
                    "W.elite-vanguard.1"
                ]
            );
            assert!(ordered(last, 3, "graveyard").is_empty());
            assert_eq!(h["counts_by_owner"]["41"]["library"], 47);
            assert_eq!(h["counts_by_owner"]["3"]["library"], 48);
            assert_eq!(
                ordered(last, 3, "library").last().unwrap(),
                "G.runeclaw-bear.1"
            );
            assert_eq!(
                membership(last, 41, "hand"),
                names(&[
                    "W.plains.5",
                    "W.plains.6",
                    "W.plains.7",
                    "W.plains.8",
                    "W.glory-seeker.1",
                    "W.silvercoat-lion.1",
                    "W.pillarfield-ox.1"
                ])
            );
            assert_eq!(
                membership(last, 3, "hand"),
                names(&["G.forest.8", "G.forest.9"])
            );
            assert_eq!(
                membership(last, 41, "battlefield"),
                names(&["W.plains.1", "W.plains.2"])
            );
            let mut gfield = names(&[
                "G.kalonian-tusker.1",
                "G.alpine-grizzly.1",
                "G.spined-wurm.1",
            ]);
            gfield.extend((1..=7).map(|n| format!("G.forest.{n}")));
            assert_eq!(membership(last, 3, "battlefield"), gfield);
            let discards = run["accepted"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| {
                    a["input"]["command"]["kind"] == "reply"
                        && a["input"]["command"]["choice"]["kind"] == "discard"
                })
                .collect::<Vec<_>>();
            assert_eq!(discards.len(), 2);
            for (index, discard) in discards.iter().enumerate() {
                assert_eq!(discard["header"]["counts_by_owner"]["41"]["hand"], 7);
                assert_eq!(
                    discard["input"]["command"]["choice"]["ordered"][0]["alias"],
                    format!("W.plains.{}", index + 3)
                );
            }
        }
        // The native journal publishes one simultaneous combat batch per damage
        // step. Its positive applied amounts are independent of terminal life.
        let mut damage = BTreeMap::<u64, Vec<String>>::new();
        for a in run["accepted"].as_array().unwrap() {
            let applied = a["current_facts"]["damage_journal"]["applied"]
                .as_array()
                .unwrap();
            if !applied.is_empty() {
                let turn = a["header"]["turn"]["position"]["number"].as_u64().unwrap();
                damage.entry(turn).or_default().extend(
                    applied
                        .iter()
                        .map(|v| v["amount"].as_str().unwrap().to_owned()),
                );
            }
        }
        for amounts in damage.values_mut() {
            amounts.sort();
        }
        let expected: Vec<(u64, Vec<&str>)> = if case == "RD01" {
            vec![
                (3, vec!["2"]),
                (5, vec!["2", "2", "2"]),
                (7, vec!["2", "2"]),
                (9, vec!["2", "2"]),
                (11, vec!["2", "2"]),
                (13, vec!["2", "2"]),
            ]
        } else {
            vec![
                (5, vec!["3"]),
                (7, vec!["2", "3", "4"]),
                (9, vec!["3", "4"]),
                (11, vec!["3", "4", "5"]),
                (13, vec!["3", "4", "5"]),
            ]
        };
        assert_eq!(
            damage,
            expected
                .into_iter()
                .map(|(turn, amounts)| (turn, amounts.into_iter().map(str::to_owned).collect()))
                .collect()
        );
    }
}

#[test]
#[ignore = "requires actual pinned catalogue metadata/gzip in explicit SPELLWAKE_TEACHING_CATALOGUE_DIR plus source-qualified vocabulary; execute separately"]
fn teaching_source_rejects_invalid_actual_intake_before_opening() {
    let vocabulary = spellwake_reference_tools::creature_types::selected_vocabulary().unwrap();
    let directory = PathBuf::from(
        std::env::var_os("SPELLWAKE_TEACHING_CATALOGUE_DIR")
            .expect("explicit actual catalogue required"),
    );
    let metadata = directory.join("oracle-metadata.json");
    let snapshot = directory.join("oracle-cards-20261029210158.jsonl.gz");
    let temp = tempfile::tempdir().unwrap();
    assert!(
        TeachingSource::load(&temp.path().join("absent.json"), &snapshot, &vocabulary).is_err()
    );
    assert!(TeachingSource::load(&metadata, &temp.path().join("absent.gz"), &vocabulary).is_err());
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
    for (name, field, value) in [
        ("size", "compressed_size", serde_json::json!(0)),
        ("kind", "type", serde_json::json!("default_cards")),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        let path = temp.path().join(format!("{name}.json"));
        std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(TeachingSource::load(&path, &snapshot, &vocabulary).is_err());
    }
    let source = source(&vocabulary);
    assert!(source.run(&[], vocabulary).is_err());
    for unknown in ["G01", "RD00", "rd01", "RD01 "] {
        assert!(TeachingCase::parse(unknown).is_err());
    }
}

#[test]
fn teaching_cli_rejects_unknown_options_and_missing_source_arguments() {
    for args in [vec!["--unknown"], vec!["--metadata"]] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_teaching-game"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("usage:") || error.contains("missing teaching argument value"),
            "{error}"
        );
    }
}
