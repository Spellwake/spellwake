//! OB01 uses synthetic native Programs and actual selected vocabulary. It exercises
//! shared observations without ordinary admission, Report3, Registry or Forge credit.
use super::{TokenChange, equal, token_changes, view};
use crate::ordinary_game::fixture::{
    A, Alias, B, CaseId, ContextInputs, Fixture, FixtureInitialization,
};
use crate::projection;
use serde::Serialize;
use serde_json::{Value, json};
use spellwake_engine::{
    characteristics::{Color, PowerToughnessCounters},
    combat::CombatRules,
    effects::SeedCard,
    game::{GameCommand, GameContext, GameId, GameInput, GameState, OpeningSetup},
    mana::ManaCost,
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{RulesJournal, RulesOccurrence, RulesRecord},
    priority::{PriorityPhase, SeatId},
    program::{BodyTarget, ExecutableBody, Instruction, Program},
    setup::GameSeed,
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    targets::TargetRef,
    token::{TokenCreation, TokenDefinition, TokenName, TokenSubtype},
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Test-only endpoint/checkpoint receipts over shared native observations.
#[derive(Clone, Serialize)]
struct Receipt {
    ordinal: usize,
    input: Option<Value>,
    before: Option<view::World>,
    after: view::World,
    checkpoint: view::World,
    token_changes: Vec<TokenChange>,
    rules_journal: Value,
}

struct Run {
    state: GameState,
    history: view::History,
    inputs: Vec<GameInput>,
    journals: Vec<RulesJournal>,
    receipts: Vec<Receipt>,
}
impl Run {
    fn initialize(fixture: &Fixture) -> Self {
        let FixtureInitialization::Scheduled(start) = fixture.initialization else {
            panic!("OB01 has an explicit scheduled synthetic premise")
        };
        let candidate = GameState::initialize_scheduled(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.seed.clone(),
            start,
        )
        .unwrap();
        let mut history = view::History::default();
        history.remember(None, &candidate);
        let after = view::world(fixture, &history, candidate.state()).unwrap();
        assert!(after.tokens.is_empty());
        let receipt = Receipt {
            ordinal: 0,
            input: None,
            before: None,
            checkpoint: after.clone(),
            after,
            token_changes: vec![],
            rules_journal: projection::journal(candidate.rules_journal()),
        };
        Self {
            journals: vec![candidate.rules_journal().clone()],
            state: candidate.into_state(),
            history,
            inputs: vec![],
            receipts: vec![receipt],
        }
    }
    fn accept_input(&mut self, fixture: &Fixture, input: GameInput) -> usize {
        let before = view::world(fixture, &self.history, &self.state).unwrap();
        let observed_input =
            serde_json::to_value(view::input(fixture, &self.history, &input).unwrap()).unwrap();
        let source = self.state.clone();
        let candidate = self.state.advance(input.clone()).unwrap();
        assert_eq!(
            self.state, source,
            "rejected and successful calls retain their source"
        );
        for record in &candidate.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::TokenCreated { after, .. } = &occurrence.value {
                        assert!(
                            self.history.known(fixture, after.object.id()).is_err(),
                            "a real new native ID is unknown before its appearance is retained"
                        );
                    }
                }
            }
        }
        self.history.remember(Some(&self.state), &candidate);
        let after = view::world(fixture, &self.history, candidate.state()).unwrap();
        let ordinal = self.inputs.len() + 1;
        self.receipts.push(Receipt {
            ordinal,
            input: Some(observed_input),
            token_changes: token_changes(&before, &after),
            before: Some(before),
            checkpoint: after.clone(),
            after,
            rules_journal: projection::journal(candidate.rules_journal()),
        });
        self.journals.push(candidate.rules_journal().clone());
        self.inputs.push(input);
        self.state = candidate.into_state();
        ordinal
    }
    fn accept(&mut self, fixture: &Fixture, actor: SeatId, command: GameCommand) -> usize {
        self.accept_input(
            fixture,
            GameInput {
                basis: self.state.basis().clone(),
                actor,
                command,
            },
        )
    }
    fn holder(&self) -> SeatId {
        assert!(
            self.state.pending_decision().is_none(),
            "OB01 invents no choice replies"
        );
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            self.state.execution().window().phase()
        else {
            panic!("OB01 requires actual ready priority")
        };
        holder
    }
    fn pass(&mut self, fixture: &Fixture) -> usize {
        self.accept(fixture, self.holder(), GameCommand::Pass)
    }
    fn cast_and_resolve(
        &mut self,
        fixture: &Fixture,
        card: CardId,
        target: Option<ObjectId>,
    ) -> usize {
        if self.holder() != A {
            self.pass(fixture);
        }
        assert_eq!(self.holder(), A);
        let spell = self
            .state
            .execution()
            .window()
            .cards()
            .current_identity(card)
            .unwrap();
        let command = match target {
            Some(target) => GameCommand::Cast {
                spell,
                target: TargetRef::Object(target),
            },
            None => GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        };
        self.accept(fixture, A, command);
        self.pass(fixture);
        let ordinal = self.pass(fixture);
        assert!(self.state.execution().window().entries().is_empty());
        ordinal
    }
}

fn fixture() -> Fixture {
    let vocabulary = crate::creature_types::selected_vocabulary()
        .expect("explicit source-qualified OB01 vocabulary");
    let zombie = vocabulary.resolve_label("Zombie").unwrap();
    let mut template = Program::creature(None, 2.into(), 2.into(), BTreeSet::from([zombie]));
    template.effect_defined_colors = Some(BTreeSet::from([Color::Black]));
    let definition = TokenDefinition::new(
        template,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(zombie)],
        },
        &vocabulary,
    )
    .unwrap();
    let programs = [
        (
            BodyTarget::None,
            Instruction::CreateTokens {
                creations: vec![TokenCreation::one(definition)],
            },
        ),
        (
            BodyTarget::SingleCreature,
            Instruction::PutPowerToughnessCounters {
                counters: PowerToughnessCounters {
                    plus_one: 2.into(),
                    minus_one: 0.into(),
                },
            },
        ),
        (
            BodyTarget::SingleCreature,
            Instruction::Damage { amount: 1.into() },
        ),
        (BodyTarget::SingleCreature, Instruction::ReturnToOwnersHand),
    ];
    let cards: Vec<_> = programs
        .into_iter()
        .enumerate()
        .map(|(index, (target, instruction))| SeedCard {
            card: CardId(u64::try_from(index).unwrap() + 1),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(target, vec![instruction]),
            ),
        })
        .collect();
    let aliases: BTreeMap<_, _> = cards
        .iter()
        .map(|row| {
            (
                row.card,
                Alias {
                    alias: format!("OB01-synthetic-spell-{}", row.card.0),
                    card: row.card,
                    owner: row.owner,
                    definition_ref: format!("OB01-synthetic-program-{}", row.card.0),
                },
            )
        })
        .collect();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context_inputs = ContextInputs {
        reference: "OB01-native-test-only".into(),
        release: "OB01-synthetic-token-observer".into(),
        rules: vocabulary.rules_revision().into(),
        creature_vocabulary: vocabulary.id(),
        name_rules_id: names.id(),
        catalogue: "synthetic-no-catalogue-admission".into(),
        programs: "OB01-four-synthetic-free-instants".into(),
        profile: "OB01-native-test-only-not-ordinary".into(),
    };
    let context = GameContext::new(
        context_inputs.release.clone(),
        context_inputs.rules.clone(),
        context_inputs.catalogue.clone(),
        context_inputs.programs.clone(),
        context_inputs.profile.clone(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    // Fixture is only the shared world's alias/context argument. No ordinary
    // admission, Header/Facts or Recorder path is invoked for these synthetic Programs.
    Fixture {
        case: CaseId::G01,
        initialization: FixtureInitialization::Scheduled(TurnStart {
            number: 1,
            step: Step::End,
        }),
        game: GameId(704111),
        context,
        context_inputs,
        setup: OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![]),
        },
        aliases,
        definitions: vec![],
        shuffles: vec![],
    }
}

fn assert_token(receipt: &Receipt, object: ObjectId, power: i64, marks: i64) {
    let token = &receipt.after.tokens[&view::opaque(object)];
    let creature = token.creature.as_ref().unwrap();
    assert_eq!(creature.power, power.into());
    assert_eq!(creature.toughness, power.into());
    assert_eq!(creature.marked_damage, marks.into());
    let serialized = serde_json::to_value(token).unwrap();
    assert_eq!(serialized["creature"]["power"], power.to_string());
    assert_eq!(serialized["creature"]["toughness"], power.to_string());
    assert_eq!(serialized["creature"]["marked_damage"], marks.to_string());
}

#[test]
fn token_marks_and_counters_survive_shared_observation_and_input_rerun() {
    let fixture = fixture();
    let mut run = Run::initialize(&fixture);
    let born = run.cast_and_resolve(&fixture, CardId(1), None);
    let births: Vec<_> = run.journals[born]
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(&event.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|occurrence| match &occurrence.value {
            RulesOccurrence::TokenCreated { after, .. } => Some(after.object.id()),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 1);
    let token = births[0];
    let key = view::opaque(token);
    assert_token(&run.receipts[born], token, 2, 0);
    let born_program = &run.receipts[born].after.tokens[&key].program;
    assert_eq!(born_program["names"], json!(["Zombie Token"]));
    assert_eq!(born_program["card_types"], json!(["Creature"]));
    assert_eq!(born_program["effect_defined_colors"], json!(["Black"]));
    assert_eq!(born_program["kind"]["casting_cost"], Value::Null);
    for family in ["mana", "activated", "triggered"] {
        assert_eq!(born_program[family], json!([]));
    }
    assert_eq!(
        run.receipts[born].after.tokens[&key]
            .counters
            .as_ref()
            .unwrap()
            .plus_one,
        0.into()
    );
    assert!(
        matches!(&run.receipts[born].token_changes[..], [TokenChange::Added { object, after }]
        if object == &key && after == &run.receipts[born].after.tokens[&key])
    );
    assert!(run.history.known(&fixture, token).unwrap().alias.is_none());
    assert!(run.history.required(&fixture, token).is_err());

    let counters = run.cast_and_resolve(&fixture, CardId(2), Some(token));
    assert_token(&run.receipts[counters], token, 4, 0);
    let counter_view = run.receipts[counters].after.tokens[&key]
        .counters
        .as_ref()
        .unwrap();
    assert_eq!(counter_view.plus_one, 2.into());
    assert_eq!(counter_view.minus_one, 0.into());
    let counter_wire = serde_json::to_value(counter_view).unwrap();
    assert_eq!(counter_wire, json!({"plus_one":"2","minus_one":"0"}));
    let damage = run.cast_and_resolve(&fixture, CardId(3), Some(token));
    assert_token(&run.receipts[damage], token, 4, 1);
    assert!(
        matches!(&run.receipts[damage].token_changes[..], [TokenChange::Updated { object, before, after }]
        if object == &key && before == &run.receipts[damage].before.as_ref().unwrap().tokens[&key]
            && after == &run.receipts[damage].after.tokens[&key])
    );

    let mut cleanup = None;
    for _ in 0..8 {
        let ordinal = run.pass(&fixture);
        if run.receipts[ordinal].after.tokens[&key]
            .creature
            .as_ref()
            .unwrap()
            .marked_damage
            == 0.into()
        {
            cleanup = Some(ordinal);
            break;
        }
    }
    let cleanup = cleanup.expect("real cleanup clears the marked damage");
    assert_token(&run.receipts[cleanup], token, 4, 0);
    assert_eq!(
        run.receipts[cleanup].after.tokens[&key]
            .counters
            .as_ref()
            .unwrap()
            .plus_one,
        2.into()
    );
    assert!(
        matches!(&run.receipts[cleanup].token_changes[..], [TokenChange::Updated { object, before, after }]
        if object == &key && before.creature.as_ref().unwrap().marked_damage == 1.into()
            && after.creature.as_ref().unwrap().marked_damage == 0.into())
    );
    assert_eq!(run.state.turn().unwrap().position().number, 2);
    assert!(
        run.receipts[cleanup]
            .rules_journal
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["cause"]["tag"] == "CleanupCharacteristics")
    );

    let removed = run.cast_and_resolve(&fixture, CardId(4), Some(token));
    assert!(run.receipts[removed].after.tokens.is_empty());
    assert!(
        matches!(&run.receipts[removed].token_changes[..], [TokenChange::Removed { object, before }]
        if object == &key && before == &run.receipts[removed].before.as_ref().unwrap().tokens[&key])
    );
    let mut moved = None;
    let mut ceased = None;
    let mut departure_order = None;
    let mut cessation_order = None;
    for (index, record) in run.journals[removed].records.iter().enumerate() {
        if let RulesRecord::Event(event) = record {
            for occurrence in &event.occurrences {
                match &occurrence.value {
                    RulesOccurrence::ZoneChange(change) if change.before.object.id() == token => {
                        assert_eq!(change.after.object.zone(), ObjectZone::Hand(A));
                        moved = Some(change.after.object.id());
                        departure_order = Some(index);
                    }
                    RulesOccurrence::TokenCeased { object } => {
                        ceased = Some(object.id());
                        cessation_order = Some(index);
                    }
                    _ => {}
                }
            }
        }
    }
    let moved = moved.expect("actual bounce journal retains the token's new hand incarnation");
    assert_ne!(moved, token);
    assert_eq!(ceased, Some(moved));
    assert!(departure_order.unwrap() < cessation_order.unwrap());
    assert!(run.history.known(&fixture, token).unwrap().alias.is_none());
    assert!(run.history.known(&fixture, moved).unwrap().alias.is_none());
    assert!(run.state.execution().window().cards().get(moved).is_none());
    assert!(
        run.receipts
            .iter()
            .skip(1)
            .any(|receipt| receipt.token_changes.is_empty())
    );
    for receipt in &run.receipts {
        assert!(equal(&receipt.after, &receipt.checkpoint).unwrap());
        assert_eq!(
            receipt.rules_journal,
            projection::journal(&run.journals[receipt.ordinal])
        );
    }

    let mut replay = Run::initialize(&fixture);
    for input in &run.inputs {
        replay.accept_input(&fixture, input.clone());
    }
    assert!(equal(&run.receipts, &replay.receipts).unwrap());
    assert_eq!(run.journals, replay.journals);
    assert_eq!(run.inputs, replay.inputs);
    assert_eq!(run.state, replay.state);
    assert!(
        equal(
            &view::world(&fixture, &run.history, &run.state).unwrap(),
            &run.receipts.last().unwrap().checkpoint
        )
        .unwrap()
    );

    let baseline = serde_json::to_value(&run.receipts[damage]).unwrap();
    let mut controls = vec![];
    for field in ["creature", "counters"] {
        let mut changed = baseline.clone();
        changed["after"]["tokens"][&key]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(!equal(&baseline, &changed).unwrap());
        controls.push(if field == "creature" {
            "missing-token-creature"
        } else {
            "missing-token-counters"
        });
    }
    for (name, path, field, replacement) in [
        ("missing-token-marks", "creature", "marked_damage", None),
        (
            "changed-token-marks",
            "creature",
            "marked_damage",
            Some(json!("0")),
        ),
        ("missing-plus-counters", "counters", "plus_one", None),
        (
            "changed-plus-counters",
            "counters",
            "plus_one",
            Some(json!("1")),
        ),
        ("missing-minus-counters", "counters", "minus_one", None),
        (
            "changed-minus-counters",
            "counters",
            "minus_one",
            Some(json!("1")),
        ),
    ] {
        let mut changed = baseline.clone();
        let object = changed["after"]["tokens"][&key][path]
            .as_object_mut()
            .unwrap();
        match replacement {
            Some(value) => {
                object.insert(field.into(), value);
            }
            None => {
                object.remove(field);
            }
        }
        assert!(!equal(&baseline, &changed).unwrap(), "{name}");
        controls.push(name);
    }
    for field in ["kind", "object", "before", "after"] {
        let mut changed = baseline.clone();
        changed["token_changes"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            !equal(&baseline, &changed).unwrap(),
            "missing updated {field}"
        );
        controls.push(match field {
            "kind" => "missing-delta-kind",
            "object" => "missing-delta-object",
            "before" => "missing-delta-before",
            "after" => "missing-delta-after",
            _ => unreachable!(),
        });
    }
    let mut changed = baseline.clone();
    changed["token_changes"] = json!([]);
    assert!(!equal(&baseline, &changed).unwrap());
    controls.push("lost-token-update");
    let mut changed = baseline.clone();
    changed["token_changes"][0]["after"]["creature"]["marked_damage"] = json!("0");
    assert!(!equal(&baseline, &changed).unwrap());
    controls.push("changed-delta-marks");
    let mut changed = baseline.clone();
    changed["token_changes"][0]["before"]["counters"]["plus_one"] = json!("1");
    assert!(!equal(&baseline, &changed).unwrap());
    controls.push("changed-delta-counters");
    let mut changed = baseline.clone();
    changed.as_object_mut().unwrap().remove("token_changes");
    assert!(!equal(&baseline, &changed).unwrap());
    controls.push("missing-token-delta-array");
    let empty = run
        .receipts
        .iter()
        .skip(1)
        .find(|receipt| receipt.token_changes.is_empty())
        .unwrap();
    let empty = serde_json::to_value(empty).unwrap();
    assert_eq!(empty["token_changes"], json!([]));
    let mut missing_empty = empty.clone();
    missing_empty
        .as_object_mut()
        .unwrap()
        .remove("token_changes");
    assert!(!equal(&empty, &missing_empty).unwrap());
    controls.push("missing-empty-token-delta-array");
    assert!(
        equal(
            &baseline,
            &serde_json::to_value(&run.receipts[damage]).unwrap()
        )
        .unwrap()
    );

    if let Some(path) = std::env::var_os("SPELLWAKE_MANDATORY_OBSERVER_RECEIPT") {
        let receipt = json!({
            "schema_version":1,
            "classification":"native-test-only-synthetic-observer-mechanism",
            "ordinary_report":false,"registry_witness":false,"forge_comparison":false,
            "creature_vocabulary_id":fixture.context_inputs.creature_vocabulary,
            "saved_input_count":run.inputs.len(),"complete_rules_journal_count":run.journals.len(),
            "observations":run.receipts,"equal_saved_input_reexecution":true,
            "equal_complete_rules_journals":true,"equal_final_native_state":true,
            "checkpoints":{"born":born,"counters":counters,"damage":damage,"cleanup":cleanup,"removed":removed},
            "effective_corruption_controls":controls,"baseline_restored":true,
        });
        std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
}
