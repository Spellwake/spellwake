//! Contract checks at actual owner boundaries, without a fabricated root.
use super::*;
use spellwake_engine::{
    combat::CombatRules,
    effects::SeedCard,
    object_identity::{CardId, ObjectDestination, ObjectZone},
    program::Program,
    setup::GameSeed,
};

const A: SeatId = SeatId(4);
const B: SeatId = SeatId(9);

fn fixture() -> EffectWindow {
    let cards = [1, 2]
        .into_iter()
        .map(|id| SeedCard {
            card: CardId(id),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::creature(None, 0.into(), 0.into(), Default::default()),
        })
        .collect();
    EffectWindow::new(GameSeed::with_uniform_life(
        vec![B, A],
        A,
        cards,
        vec![CardId(1), CardId(2)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        super::super::creature_types::synthetic_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    ))
    .unwrap()
}

#[test]
fn executor_views_keep_real_pending_domain_and_completed_move_before_receipts() {
    let before = fixture();
    let aliases =
        AliasContext::from_card_aliases(&[(CardId(1), "One".into()), (CardId(2), "Two".into())])
            .unwrap();
    let pending = before.begin_checks(before.revision()).unwrap();
    let view = rule_progress(&pending, &aliases).unwrap();
    assert_eq!(view["result"]["tag"], "Pending");
    assert_eq!(view["result"]["frame"]["tag"], "Checks");
    assert_eq!(view["result"]["frame"]["frame"]["kind"], "graveyard_order");
    assert_eq!(
        view["result"]["frame"]["frame"]["domain"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(view["append"], json!([]));
    assert!(view["state"].get("basis").is_none());
    assert!(view["state"].get("decision").is_none());
    let RuleResult::Pending { frame, .. } = pending.result() else {
        panic!("missing actual order frame")
    };
    let mut ordered = before
        .window()
        .cards()
        .iter()
        .map(GameObject::id)
        .collect::<Vec<_>>();
    ordered.reverse();
    let completed = pending
        .state()
        .resume_rule(
            pending.state().revision(),
            frame,
            A,
            DecisionReply::OrderGraveyard(ordered.clone()),
            None,
        )
        .unwrap();
    let view = rule_progress(&completed, &aliases).unwrap();
    assert_eq!(view["result"]["tag"], "Complete");
    assert_eq!(view["result"]["completion"]["tag"], "Checks");
    assert_eq!(view["result"]["completion"]["performed"], true);
    assert_eq!(view["result"]["completion"]["waiting_triggers"], false);
    assert_eq!(
        view["result"]["checks"]["moves"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        view["result"]["checks"]["choices"][0]["ordered"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        ordered
            .iter()
            .all(|id| completed.state().window().cards().get(*id).is_none())
    );
    assert!(
        before
            .window()
            .cards()
            .iter()
            .all(|card| card.zone() == ObjectZone::Battlefield)
    );
    assert!(
        completed
            .state()
            .window()
            .cards()
            .iter()
            .all(|card| card.zone() == ObjectZone::Graveyard(A))
    );
}

#[test]
fn raw_owner_views_retain_participant_order_without_executor_body_or_root_fields() {
    let execution = fixture();
    let value = stack_window(execution.window());
    assert_eq!(value["participants"], json!([9, 4]));
    assert_eq!(value["active"], 4);
    assert_eq!(
        value["phase"],
        json!({"tag":"Priority","phase":{"tag":"RequiresChecks","candidate":4}})
    );
    assert_eq!(value["cards"]["piles"][0]["player"], 9);
    assert_eq!(value["cards"]["piles"][1]["player"], 4);
    assert_eq!(value["cards"]["objects"].as_array().unwrap().len(), 2);
    assert!(value.get("programs").is_none());
    assert!(value.get("basis").is_none());
    assert!(value.get("payloads").is_none());
}

#[test]
fn current_root_projection_retains_all_locals_and_chosen_occurrences_after_completion() {
    use spellwake_engine::{
        game::{GameContext, GameId},
        mana::ManaCost,
        program::{
            BodyTarget, CreatureTypeSlot, ExecutableBody, Instruction, ObjectFilter, ProgramKind,
            SpellDefinition, ValueExpr,
        },
    };
    let vocabulary = super::super::creature_types::synthetic_vocabulary();
    let first = vocabulary.resolve_label("Elf").unwrap();
    let second = vocabulary.resolve_label("Cat").unwrap();
    let program = Program {
        cast_reductions: vec![],
        entry_counter_abilities: vec![],
        conditional_cost: None,
        plot: None,
        adventure: None,
        splice: None,
        entry_copy: None,
        nightbound_back: None,
        transform_back: None,
        split: None,
        madness: None,
        miracle: None,
        cast_from_own_exile: false,
        exile_action: None,
        flashback: None,
        flashback_x_mana_symbols: 0,
        sacrifice_creature_cast_cost: false,
        sacrifice_cast_cost_filter: None,
        tap_creatures_cast_cost: false,
        reveal_colorless_creature_cast_cost: false,
        discard_cast_cost: false,
        discard_cast_cost_filter: None,
        graveyard_creature_exile_cost: 0,
        graveyard_exile_x_cost: false,
        graveyard_self_cast: None,
        flashback_sacrifice_creatures: 0,
        flashback_tap_white_creatures: 0,
        flashback_life: 0,
        additional_cast_costs: vec![],
        activation_restrictions: Default::default(),
        activation_hybrid_symbols: Default::default(),
        loyalty_costs: Default::default(),
        variable_loyalty_costs: Default::default(),
        face_up_abilities: Vec::new(),
        printed_keywords: Default::default(),
        printed_toxic: None,
        x_mana_symbols: 0,
        hybrid_symbols: Vec::new(),
        cannot_be_countered: false,
        attack_requirement: None,
        combat_limits: Default::default(),
        cast_during_own_combat: false,
        additional_life_cost: None,
        siege_back: None,
        disturb: None,
        meld_partner: None,
        printed_defense: None,
        printed_loyalty: None,
        zone_replacements: vec![],
        card_types: std::collections::BTreeSet::from([
            spellwake_engine::program::CardType::Instant,
        ]),
        creature_subtypes: std::collections::BTreeSet::new(),
        land_subtypes: std::collections::BTreeSet::new(),
        spell_subtypes: std::collections::BTreeSet::new(),
        artifact_subtypes: std::collections::BTreeSet::new(),
        enchantment_subtypes: std::collections::BTreeSet::new(),
        planeswalker_subtypes: Default::default(),
        names: spellwake_contracts::names::ObjectNames::nameless(),
        supertypes: std::collections::BTreeSet::new(),
        color_indicator: None,
        effect_defined_colors: None,
        kind: ProgramKind::Spell(SpellDefinition {
            modes: None,
            cost: ManaCost {
                generic: 0.into(),
                required: Default::default(),
            },
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ChooseCreatureType {
                        into: CreatureTypeSlot::new(0),
                        excluding: Default::default(),
                    },
                    Instruction::DrawController {
                        amount: ValueExpr::Literal(0.into()),
                    },
                    Instruction::ChooseCreatureType {
                        into: CreatureTypeSlot::new(1),
                        excluding: Default::default(),
                    },
                    Instruction::DrawController {
                        amount: ValueExpr::count_battlefield(
                            ObjectFilter::any()
                                .controlled()
                                .of_chosen_slot(CreatureTypeSlot::new(0)),
                        ),
                    },
                ],
            ),
        }),
        mana: vec![],
        activated: vec![],
        triggered: vec![],
        continuous: None,
        additional_battlefield_abilities: Vec::new(),
    };
    let context = GameContext::new(
        "synthetic".into(),
        vocabulary.rules_revision().into(),
        "synthetic".into(),
        "synthetic".into(),
        "synthetic".into(),
        vocabulary.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program,
        }],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary.clone(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let mut root = GameState::initialize(GameId(1), context, seed)
        .unwrap()
        .state()
        .clone();
    fn advance(state: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
        state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            })
            .unwrap()
    }
    let spell = root
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    root = advance(
        &root,
        A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    )
    .state()
    .clone();
    root = advance(&root, A, GameCommand::Pass).state().clone();
    root = advance(&root, B, GameCommand::Pass).state().clone();
    let aliases = AliasContext::default();
    let decision = root.pending_decision().unwrap();
    let first_frame = frame(decision.frame(), &root).unwrap();
    assert_eq!(first_frame["tag"], "CreatureType");
    assert_eq!(first_frame["slot"], "0");
    assert_eq!(first_frame["vocabulary"], vocabulary.id().to_string());
    let first_result = advance(
        &root,
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::CreatureType(first),
        },
    );
    root = first_result.state().clone();
    let projected = state(&root, &aliases).unwrap();
    assert_eq!(
        projected["basis"]["context"]["creature_vocabulary"],
        vocabulary.id().to_string()
    );
    assert_eq!(projected["resolution_state"]["instruction"], "2");
    assert_eq!(
        projected["resolution_state"]["locals"],
        json!([{"slot":"0","value":creature_type_id(first)}])
    );
    assert_eq!(projected["decision"]["frame"]["slot"], "1");
    assert_eq!(
        projected["programs"][0]["definition"]["kind"]["body"]["instructions"][3]["amount"]["tag"],
        "ControlledPermanentsOfChosenType"
    );
    let decision = root.pending_decision().unwrap();
    let completed = advance(
        &root,
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::CreatureType(second),
        },
    );
    let projected = candidate(&completed, Some(&root), &aliases).unwrap();
    assert!(projected["state"]["resolution_state"].is_null());
    let chosen = completed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(value) => Some(&value.occurrences),
            _ => None,
        })
        .flatten()
        .filter_map(|row| match &row.value {
            RulesOccurrence::CreatureTypeChosen { value, .. } => Some(*value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(chosen, vec![second]);
    let serialized = journal(completed.rules_journal());
    assert!(serialized.as_array().unwrap().iter().any(|event| {
        event["occurrences"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["value"]["value"] == creature_type_id(second))
        })
    }));
}

#[test]
fn world_seed_input_preserves_ordered_and_equal_cohorts_separately_from_battlefield_order() {
    use spellwake_engine::program::Supertype;
    let mut world = Program::artifact(None, vec![]);
    world.supertypes.insert(Supertype::World);
    let mut seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![
            SeedCard {
                card: CardId(11),
                owner: A,
                destination: ObjectDestination::Battlefield { controller: A },
                program: world.clone(),
            },
            SeedCard {
                card: CardId(22),
                owner: B,
                destination: ObjectDestination::Battlefield { controller: B },
                program: world.clone(),
            },
            SeedCard {
                card: CardId(33),
                owner: B,
                destination: ObjectDestination::Battlefield { controller: B },
                program: world,
            },
        ],
        vec![CardId(33), CardId(22), CardId(11)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        super::super::creature_types::synthetic_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    seed.initial_world_cohorts = vec![vec![CardId(11)], vec![CardId(22), CardId(33)]];
    let input = game_seed_input(&seed);
    assert_eq!(
        input["initial_world_cohorts"],
        json!([["11"], ["22", "33"]])
    );
    assert_eq!(
        input["initial_battlefield_order"],
        json!(["33", "22", "11"])
    );
    let mut replay_seed = seed.clone();
    let groups: Vec<Vec<String>> =
        serde_json::from_value(input["initial_world_cohorts"].clone()).unwrap();
    replay_seed.initial_world_cohorts = groups
        .into_iter()
        .map(|g| {
            g.into_iter()
                .map(|id| CardId(id.parse().unwrap()))
                .collect()
        })
        .collect();
    let original = EffectWindow::new(seed).unwrap();
    let replayed = EffectWindow::new(replay_seed).unwrap();
    assert_eq!(original, replayed);
    assert_eq!(
        replayed
            .world_tenures()
            .map(|(_, tenure)| tenure.sequence())
            .collect::<Vec<_>>(),
        vec![1, 2, 2]
    );
}
