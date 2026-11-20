//! Source-qualified semantic observations for normal native/Forge comparison.
//! Both use prepared state and injected G funding; no opening/full-card claim.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod trajectories;
use serde::Serialize;
use spellwake_engine::{
    characteristics::Color,
    combat::CombatRules,
    effects::SeedCard,
    game::{
        DecisionReply, GameCommand, GameContext, GameId, GameState, NativeCandidate, RuleFrame,
    },
    life::LifeAdjustment,
    mana::{FloatingMana, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord, TriggerRelated},
    priority::PriorityPhase,
    program::{
        BodyTarget, ExecutableBody, Instruction, InstructionAmount, ObjectRelation,
        OrdinaryCondition, Program, TriggeredAbility, TriggeredAbilityId,
    },
    setup::GameSeed,
    stack_window::{StackKind, WindowPhase},
    token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients, TokenSubtype},
    turn::{Step, TurnStart},
};
use std::{collections::BTreeSet, io::Write, sync::Arc};
use trajectories::{P0, P1, object, reply, submit, vocabulary};
#[derive(Serialize)]
struct TokenFact {
    id: String,
    owner: u32,
    controller: Option<u32>,
    token: bool,
    names: Vec<String>,
    types: Vec<String>,
    subtypes: Vec<String>,
    power: String,
    toughness: String,
}
#[derive(Serialize)]
struct Pair {
    source: String,
    related: String,
}
#[derive(Serialize, Default)]
struct Facts {
    births: Vec<serde_json::Value>,
    pairs: Vec<Pair>,
    choices: Vec<serde_json::Value>,
    trigger_bodies: u32,
    groups: Vec<serde_json::Value>,
    draws: usize,
}
fn retain(candidate: &NativeCandidate, facts: &mut Facts) {
    facts.draws += candidate.draw_journal().events.len();
    for record in &candidate.rules_journal().records {
        match record {
            RulesRecord::Event(event) => {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::TokenCreated { creator, after } = &occurrence.value {
                        facts.births.push(serde_json::json!({"event":event.id.sequence().to_string(),"index":occurrence.id.index.to_string(),"creator":creator.0,"object":format!("{:?}",after.object.id())}));
                    }
                }
            }
            RulesRecord::TriggerDiscovered(w) => {
                facts.groups.push(serde_json::json!({"instance":w.id.sequence().to_string(),"source":format!("{:?}",w.source.object.id()),"controller":w.controller.0,"definition":w.definition.0,"event":w.causes.event().sequence().to_string(),"key":format!("{:?}",w.causes.key()),"members":w.causes.members().iter().map(|member|serde_json::json!({"event":member.occurrence.event.sequence().to_string(),"index":member.occurrence.index.to_string(),"related_player":member.related_player.map(|p|p.0),"related_token":match &member.related{TriggerRelated::TokenCreated{after,..}=>Some(format!("{:?}",after.object.id())),_=>None}})).collect::<Vec<_>>()}));
                for member in w.causes.members() {
                    if let TriggerRelated::TokenCreated { after, .. } = &member.related {
                        facts.pairs.push(Pair {
                            source: format!("{:?}", w.source.object.id()),
                            related: format!("{:?}", after.object.id()),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}
fn resolve(state: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = state.execution().window().phase()
    else {
        panic!("real priority required")
    };
    let first = submit(state, holder, GameCommand::Pass);
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
        first.state().execution().window().phase()
    else {
        panic!("second real priority required")
    };
    submit(first.state(), holder, GameCommand::Pass)
}
fn grouped(scenario: &str) -> bool {
    matches!(scenario, "entry" | "created" | "count" | "separate")
}
fn trigger(scenario: &str) -> TriggeredAbility {
    TriggeredAbility {
        grouping: if grouped(scenario) {
            spellwake_engine::program::TriggerGrouping::OneOrMoreTogether
        } else {
            spellwake_engine::program::TriggerGrouping::EachOccurrence
        },
        id: TriggeredAbilityId(7),
        condition: if scenario == "created" {
            OrdinaryCondition::TokenCreated(spellwake_engine::program::PlayerRelation::Controller)
        } else {
            OrdinaryCondition::Enters(if grouped(scenario) {
                ObjectRelation::ControlledCreature
            } else {
                ObjectRelation::AnyCreature
            })
        },
        body: ExecutableBody::new(
            BodyTarget::None,
            if scenario == "count" {
                vec![Instruction::DrawController {
                    amount: InstructionAmount::MatchedOccurrences,
                }]
            } else {
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }]
            },
        ),
    }
}
fn run(scenario: &str) {
    let v = vocabulary();
    let saproling = v.resolve_label("Saproling").unwrap();
    let mut token = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([saproling]));
    token.effect_defined_colors = Some(BTreeSet::from([Color::Green]));
    if scenario == "newcomers" {
        token.triggered.push(trigger(scenario));
    }
    let definition = TokenDefinition::new(
        token,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(saproling)],
        },
        &v,
    )
    .unwrap();
    let mut instructions = vec![Instruction::CreateTokens {
        creations: vec![TokenCreation {
            definition,
            amount: InstructionAmount::Literal(if scenario == "zero" { 0 } else { 2 }.into()),
            recipients: if scenario == "each" {
                TokenRecipients::EachPlayer
            } else {
                TokenRecipients::Controller
            },
        }],
    }];
    if scenario == "separate" {
        instructions.push(instructions[0].clone());
    }
    if scenario == "zero" {
        instructions.push(Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(3.into()),
        });
    }
    let producer = Program::instant(
        ManaCost::specific(ManaType::Green, 1.into()),
        ExecutableBody::new(BodyTarget::None, instructions),
    );
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: P0,
        destination: ObjectDestination::Hand,
        program: producer,
    }];
    let mut order = vec![];
    if scenario == "watcher" || grouped(scenario) {
        let mut watcher = Program::artifact(None, vec![]);
        watcher.triggered.push(trigger(scenario));
        cards.push(SeedCard {
            card: CardId(2),
            owner: P0,
            destination: ObjectDestination::Battlefield { controller: P0 },
            program: watcher,
        });
        order.push(CardId(2));
    }
    for (player, start) in [(P0, 100), (P1, 200)] {
        for n in 0..24 {
            cards.push(SeedCard {
                card: CardId(start + n),
                owner: player,
                destination: ObjectDestination::LibraryBottom,
                program: Program::artifact(None, vec![]),
            });
        }
    }
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "literal-reference".into(),
        v.rules_revision().into(),
        "saproling-defined-mechanism".into(),
        "simultaneous".into(),
        scenario.into(),
        v.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![P0, P1],
        P0,
        cards,
        order,
        vec![FloatingMana {
            player: P0,
            mana_type: ManaType::Green,
            amount: 1.into(),
        }],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        v.clone(),
        names,
    );
    let state = GameState::initialize_scheduled(
        GameId(202611026137),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let cast = submit(
        &state,
        P0,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    );
    assert_eq!(
        cast.mana_payment().unwrap().cost,
        ManaCost::specific(ManaType::Green, 1.into())
    );
    assert_eq!(cast.state().execution().mana(P0, ManaType::Green), 0.into());
    let casts = cast
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            if let RulesRecord::Event(event) = record {
                Some(event)
            } else {
                None
            }
        })
        .flat_map(|event| &event.occurrences)
        .filter(|occurrence| matches!(occurrence.value, RulesOccurrence::CastCompleted(_)))
        .count();
    let mana_payments = usize::from(cast.mana_payment().is_some());
    assert_eq!((casts, mana_payments), (1, 1));
    let mut facts = Facts::default();
    let completed = resolve(cast.state());
    retain(&completed, &mut facts);
    let mut state = completed.into_state();
    for _ in 0..2 {
        let Some(decision) = state.pending_decision() else {
            break;
        };
        let RuleFrame::TimestampOrder(frame) = decision.frame() else {
            break;
        };
        let offered = frame.members().iter().map(|m| m.member).collect::<Vec<_>>();
        facts.choices.push(serde_json::json!({"kind":"TimestampOrder","chooser":frame.chooser().0,"entry":format!("{:?}",frame.entry()),"instruction":frame.instruction(),"members":offered.iter().map(|m|m.ordinal()).collect::<Vec<_>>()}));
        let next = reply(
            &state,
            decision.chooser(),
            DecisionReply::TimestampOrder(offered),
        );
        retain(&next, &mut facts);
        state = next.into_state();
    }
    if let Some(decision) = state.pending_decision() {
        let RuleFrame::Triggers(frame) = decision.frame() else {
            panic!("only actual ordinary trigger ordering expected")
        };
        facts.choices.push(serde_json::json!({"kind":"OrderTriggers","chooser":frame.chooser().0,"instances":frame.instances().iter().map(|id|id.sequence().to_string()).collect::<Vec<_>>()}));
        let next = reply(
            &state,
            decision.chooser(),
            DecisionReply::OrderTriggers(frame.instances().to_vec()),
        );
        retain(&next, &mut facts);
        state = next.into_state();
    }
    for _ in 0..4 {
        if state.execution().window().entries().is_empty() {
            break;
        }
        assert!(matches!(
            state.execution().window().entries().last().unwrap().kind(),
            StackKind::TriggeredAbility { .. }
        ));
        let next = resolve(&state);
        retain(&next, &mut facts);
        facts.trigger_bodies += 1;
        state = next.into_state();
    }
    assert!(state.pending_decision().is_none());
    assert!(state.execution().window().entries().is_empty());
    let current = state.execution().evaluated_characteristics().unwrap();
    let tokens = state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.is_token())
        .map(|object| {
            assert_eq!(object.zone(), ObjectZone::Battlefield);
            let c = current.get(object.id()).unwrap();
            let pt = c.power_toughness.as_ref().unwrap();
            TokenFact {
                id: format!("{:?}", object.id()),
                owner: object.owner().0,
                controller: object.controller().map(|s| s.0),
                token: true,
                names: c
                    .current_names
                    .iter()
                    .map(|n| n.as_str().to_owned())
                    .collect(),
                types: c.current_types.iter().map(|t| format!("{t:?}")).collect(),
                subtypes: c
                    .current_creature_subtypes
                    .iter()
                    .map(|t| v.label(*t).unwrap().to_owned())
                    .collect(),
                power: String::from(pt.derived.power.clone()),
                toughness: String::from(pt.derived.toughness.clone()),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tokens.len(),
        if scenario == "zero" {
            0
        } else if scenario == "each" || scenario == "separate" {
            4
        } else {
            2
        }
    );
    assert_eq!(
        facts.trigger_bodies,
        if grouped(scenario) {
            if scenario == "separate" { 2 } else { 1 }
        } else if scenario == "watcher" {
            2
        } else if scenario == "newcomers" {
            4
        } else {
            0
        }
    );
    let expected_life = if grouped(scenario) {
        if scenario == "count" {
            20
        } else if scenario == "separate" {
            22
        } else {
            21
        }
    } else if scenario == "watcher" {
        22
    } else if scenario == "newcomers" {
        24
    } else if scenario == "zero" {
        23
    } else {
        20
    };
    assert_eq!(
        state.execution().life_total(P0),
        Some(&expected_life.into())
    );
    if grouped(scenario) {
        assert_eq!(
            facts.groups.len(),
            if scenario == "separate" { 2 } else { 1 }
        );
        assert!(
            facts
                .groups
                .iter()
                .all(|g| g["key"] == "Together" && g["members"].as_array().unwrap().len() == 2)
        );
        assert_eq!(facts.draws, if scenario == "count" { 2 } else { 0 });
    }
    let evidence_variable = if grouped(scenario) {
        "SPELLWAKE_TRIGGER_GROUPING_EVIDENCE_DIR"
    } else {
        "SPELLWAKE_SIMULTANEOUS_EVIDENCE_DIR"
    };
    if let Some(directory) = std::env::var_os(evidence_variable) {
        let path = std::path::PathBuf::from(directory).join(format!("{scenario}.json"));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        let report = serde_json::json!({"schema_version":1,"kind":"mechanism-reference-semantic-observations","scenario":scenario,"assertions_passed":true,"casts":casts,"mana_payments":mana_payments,"life_actor":state.execution().life_total(P0),"tokens":tokens,"facts":facts,"timestamps":state.execution().characteristic_timestamps().map(|(object,timestamp)|serde_json::json!({"object":format!("{object:?}"),"timestamp":timestamp.sequence().to_string()})).collect::<Vec<_>>(),"vocabulary":v.id().to_string(),"limits":"Prepared turn-one main and injected G; real root transitions, not opening/full-card/format/runtime/replay/witness/capacity qualification. Raw receipt/registry/time orders retained; cross-engine numeric identities are not joined."});
        file.write_all(&serde_json::to_vec(&report).unwrap())
            .unwrap();
        file.write_all(b"\n").unwrap();
        file.flush().unwrap();
    }
}
#[test]
fn native_normal_plural_watcher_observations() {
    run("watcher");
}
#[test]
fn native_normal_newcomer_cross_observations() {
    run("newcomers");
}
#[test]
fn native_normal_zero_then_life_observations() {
    run("zero");
}
#[test]
fn native_normal_each_player_observations() {
    run("each");
}

#[test]
fn native_grouped_entry_observations() {
    run("entry");
}
#[test]
fn native_grouped_created_observations() {
    run("created");
}
#[test]
fn native_grouped_count_observations() {
    run("count");
}
#[test]
fn native_grouped_separate_observations() {
    run("separate");
}
