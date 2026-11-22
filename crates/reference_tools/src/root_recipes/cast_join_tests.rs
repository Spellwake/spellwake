//! Actual root receipt adapter with explicit caller-supplied captured handles.
//! This is a unit boundary check, not a new external recipe binding or sealed reissue.
use super::*;
use spellwake_engine::{
    life::LifeAdjustment,
    mana::ManaCost,
    program::{
        BodyTarget, ExecutableBody, Instruction, PlayerRelation, Program, TriggerGrouping,
        TriggeredAbility, TriggeredAbilityId,
    },
};

fn cast_join_packet() -> Value {
    let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    let producer = Program::instant(ManaCost::free(), body.clone());
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(7),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            PlayerRelation::Controller,
            spellwake_engine::program::SpellFilter::any(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body,
    });
    let raw = json!({"schema_version":10,"creature_vocabulary":projection::vocabulary(&vocabulary),"kind":"deterministic-input-only-producer-recipes","no_expected_outputs":true,
        "definitions":{"producer":{"native_program":program::view(&producer)},"watcher":{"native_program":program::view(&watcher)}},
        "fixtures":[{"id":"cast-join-unit","scenario":"cast-join-unit","scope":"root_unscheduled","forge_route":"descriptor_only",
        "seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"game_id":6012,"context":["unit",vocabulary.rules_revision(),"literal","cast-join","prepared"],"creature_vocabulary":vocabulary.id().to_string(),"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"Interaction"},"floating_mana":[],"cards":[{"alias":"P","card_id":1,"owner":"A","controller":null,"zone":"Hand","definition":"producer"},{"alias":"W","card_id":2,"owner":"A","controller":"A","zone":"Battlefield","tapped":false,"definition":"watcher"}],"initial_world_cohorts":[],"initial_battlefield_order":["W"],"library_order":{"A":[],"B":[]}},
        "actions":[],"bindings":[{"name":"cast_trigger","when":"first_matching_actual_discovery_in_this_recipe","join":{"source_object_ref":"watcher","triggered_ability_id":7,"related_object_ref":"spell","require_unique":true}}],"choice_tables":[],"stop":{"kind":"Ready","after_action_ordinal":0,"no_extra_pass":true},"notes":[],"card_id_base":0,"engine_recipes":{"native":{"action_ordinals":[],"budget":{"max_root_inputs":8,"max_private_owner_calls":8}},"forge":{}}}]});
    raw
}

#[test]
fn root_discovery_join_selects_the_captured_stack_identity_and_preserves_first_binding() {
    let raw = cast_join_packet();
    let selected = input::parse_synthetic(
        &serde_json::to_vec(&raw).unwrap(),
        "cast-join-unit",
        crate::packet::SeedMode::PreparedRoot,
    )
    .unwrap();
    let game = selected.initialize().unwrap().unwrap().into_state();
    let hand = game
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let source = game
        .execution()
        .window()
        .cards()
        .current_identity(CardId(2))
        .unwrap();
    let cast = game
        .advance(GameInput {
            basis: game.basis().clone(),
            actor: input::seat("A").unwrap(),
            command: GameCommand::CastUntargeted {
                spell: hand,
                payment: None,
            },
        })
        .unwrap();
    let stack = cast
        .state()
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    assert_ne!(hand, stack);
    let joins = |related| Bindings {
        objects: BTreeMap::from([("watcher".into(), source), ("spell".into(), related)]),
        ..Bindings::default()
    };
    let mut stale = joins(hand);
    stale.observe(&selected, &cast).unwrap();
    assert!(stale.instances.is_empty());
    let mut actual = joins(stack);
    actual.observe(&selected, &cast).unwrap();
    assert_eq!(actual.instances.len(), 1);
    let first = actual.instances.clone();
    actual.observe(&selected, &cast).unwrap();
    assert_eq!(actual.instances, first);
    let mut missing = joins(stack);
    missing.objects.remove("spell");
    assert_eq!(
        missing.observe(&selected, &cast),
        Err("missing related object spell".into())
    );
}

#[test]
fn actual_root_recipe_target_reply_and_frame_guard_use_existing_player_resolver() {
    let mut raw = cast_join_packet();
    // Encode the body through the authoritative codec instead of trusting a hand-written shape.
    raw["definitions"]["watcher"]["native_program"]["triggered"][0]["body"] = program::view(&{
        let mut program = Program::artifact(None, vec![]);
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(7),
            condition: spellwake_engine::program::TriggerSpec::spell_cast(
                PlayerRelation::Controller,
                spellwake_engine::program::SpellFilter::any(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
        program
    })["triggered"][0]["body"]
        .clone();
    let selected = input::parse_synthetic(
        &serde_json::to_vec(&raw).unwrap(),
        "cast-join-unit",
        crate::packet::SeedMode::PreparedRoot,
    )
    .unwrap();
    let initial = selected.initialize().unwrap().unwrap();
    let game = initial.state();
    let spell = game
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let cast = game
        .advance(GameInput {
            basis: game.basis().clone(),
            actor: input::seat("A").unwrap(),
            command: GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        })
        .unwrap();
    let state = cast.state().clone();
    let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
        panic!("actual target frame")
    };
    let mut bindings = Bindings::default();
    bindings.instances.insert("t".into(), frame.instance());
    bindings.entries.insert("entry".into(), frame.entry());
    let driver = Driver {
        selected: &selected,
        capture: None,
        current_pointer: "/unit".into(),
        state: state.clone(),
        bindings,
        snapshots: BTreeMap::new(),
        saved: vec![],
        records: vec![],
        count: 0,
        last_candidate: cast,
        literal_replies: BTreeMap::new(),
    };
    validate_choice("TriggerTarget", &[json!({"player":"B"})]).unwrap();
    let action:Action=serde_json::from_value(json!({"op":"reply_exact_current_frame","actor":"A","kind":"TriggerTarget","items":[{"player":"B"}],"token":"actual_current_decision_id_and_basis"})).unwrap();
    let input = driver.build(&action, &state).unwrap();
    assert!(
        matches!(input.command,GameCommand::Reply{choice:DecisionReply::TriggerTarget(spellwake_engine::targets::TargetRef::Player(player)),..} if player==input::seat("B").unwrap())
    );
    let native = state.advance(input).unwrap();
    assert!(native.state().pending_decision().is_none());
    let guard:input::FrameGuard=serde_json::from_value(json!({"kind":"TriggerTarget","chooser":"A","instance_ref":"t","entry_ref":"entry","domain":[{"player":"A"},{"player":"B"}]})).unwrap();
    driver.frame_guard(&state, &guard).unwrap();
    // FrameGuard is intake-only; a differently ordered concrete guard must fail.
    let wrong_guard:input::FrameGuard=serde_json::from_value(json!({"kind":"TriggerTarget","chooser":"A","instance_ref":"t","entry_ref":"entry","domain":[{"player":"B"},{"player":"A"}]})).unwrap();
    assert!(driver.frame_guard(&state, &wrong_guard).is_err());
    assert!(validate_choice("TriggerTarget", &[]).is_err());
    assert!(
        validate_choice(
            "TriggerTarget",
            &[json!({"player":"B","captured_object":"mixed"})]
        )
        .is_err()
    );
}
