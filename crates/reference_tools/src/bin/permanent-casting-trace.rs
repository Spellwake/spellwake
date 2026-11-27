//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! Actual root inputs and current-candidate evidence for authored permanent spells.
//! No expected/reference file is read. Later mana is produced by disclosed extra
//! native actions, independently of the reference callback's trusted pool seeds.
use serde::Serialize;
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::{Instruction, Program, SeedCard},
    game::{GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate},
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment,
        ManaPaymentRecord, ManaProductionRecord, ManaSpend, ManaType,
    },
    object_identity::{CardId, GameObject, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackKind, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::collections::BTreeMap;

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const SPELL: CardId = CardId(1);
const RESPONSE: CardId = CardId(2);
const TARGET: CardId = CardId(3);
const RESOURCE: CardId = CardId(4);
type Pool = BTreeMap<&'static str, RulesInteger>;

#[derive(Serialize)]
struct Row {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,
    id: &'static str,
    kind: &'static str,
    turn: u64,
    step: &'static str,
    active: &'static str,
    actor: &'static str,
    priority: &'static str,
    stack_size: usize,
    spell_zone: &'static str,
    spell_owner: &'static str,
    spell_controller: Option<&'static str>,
    spell_tapped: Option<bool>,
    spell_power: Option<i64>,
    spell_toughness: Option<i64>,
    response_zone: Option<&'static str>,
    target_zone: Option<&'static str>,
    target_damage: Option<i64>,
    target_toughness: Option<i64>,
    mana: BTreeMap<&'static str, Pool>,
    protocol: &'static str,
}
#[derive(Serialize)]
struct Physical {
    card: u64,
    object: String,
    zone: &'static str,
    owner: &'static str,
    controller: Option<&'static str>,
    tapped: Option<bool>,
}
#[derive(Serialize)]
struct EntryEvidence {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,
    scenario: &'static str,
    action: &'static str,
    entry: String,
    before: Physical,
    after_before_checks: Physical,
    current_after_checks: Physical,
}
#[derive(Serialize)]
struct Payment {
    source: String,
    payer: &'static str,
    generic: RulesInteger,
    required: Pool,
    spend: Pool,
}
#[derive(Serialize)]
struct Production {
    source: String,
    ability: serde_json::Value,
    controller: &'static str,
    produced: Pool,
}
#[derive(Serialize)]
struct CurrentFacts {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,
    scenario: &'static str,
    action: &'static str,
    actor: &'static str,
    command: String,
    input_revision: u64,
    next_revision: u64,
    input_unchanged: bool,
    permanent_entry_present: bool,
    payment: Option<Payment>,
    production: Option<Production>,
    activation_present: bool,
    damage_attempts: usize,
    positive_damage: usize,
}
#[derive(Serialize)]
struct NativeOnlyAction {
    #[serde(
        rename = "__current_capture_point",
        skip_serializing_if = "Option::is_none"
    )]
    capture_point: Option<String>,
    scenario: &'static str,
    action: &'static str,
    reason: &'static str,
    source_after: Physical,
    mana_after: BTreeMap<&'static str, Pool>,
}
#[derive(Default)]
struct Recorder {
    current_facts: Vec<CurrentFacts>,
    permanent_entries: Vec<EntryEvidence>,
    native_only_actions: Vec<NativeOnlyAction>,
    spell_zone_paths: BTreeMap<&'static str, Vec<String>>,
    response_zone_path: Vec<String>,
    action_counts: BTreeMap<&'static str, usize>,
}

fn seat(value: SeatId) -> &'static str {
    match value {
        A => "A",
        B => "B",
        _ => panic!("unexpected fixture seat"),
    }
}
fn symbol(value: ManaType) -> &'static str {
    match value {
        ManaType::White => "W",
        ManaType::Blue => "U",
        ManaType::Black => "B",
        ManaType::Red => "R",
        ManaType::Green => "G",
        ManaType::Colourless => "C",
    }
}
fn amounts(value: &BTreeMap<ManaType, RulesInteger>) -> Pool {
    value
        .iter()
        .map(|(kind, amount)| (symbol(*kind), amount.clone()))
        .collect()
}
fn pools(state: &GameState) -> BTreeMap<&'static str, Pool> {
    [A, B]
        .into_iter()
        .map(|player| {
            (
                seat(player),
                ManaType::ALL
                    .into_iter()
                    .map(|kind| (symbol(kind), state.execution().mana(player, kind)))
                    .collect(),
            )
        })
        .collect()
}
fn zone(value: ObjectZone) -> &'static str {
    match value {
        ObjectZone::Hand(_) => "Hand",
        ObjectZone::Stack => "Stack",
        ObjectZone::Battlefield => "Battlefield",
        ObjectZone::Graveyard(_) => "Graveyard",
        _ => panic!("unexpected fixture zone"),
    }
}
fn physical(value: &GameObject) -> Physical {
    Physical {
        card: value.card_id().expect("source-authored card-only trace").0,
        object: format!("{:?}", value.id()),
        zone: zone(value.zone()),
        owner: seat(value.owner()),
        controller: value.controller().map(seat),
        tapped: value.tapped(),
    }
}
fn object(state: &GameState, card: CardId) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(card)
        .expect("fixture card remains physically registered")
}
fn card(state: &GameState, id: CardId) -> &GameObject {
    state
        .execution()
        .window()
        .cards()
        .get(object(state, id))
        .expect("current fixture object exists")
}
fn holder(state: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = state.execution().window().phase()
    else {
        panic!("fixture observation requires actual ready priority")
    };
    holder
}
fn bounded(value: RulesInteger) -> i64 {
    String::from(value)
        .parse()
        .expect("fixture PT and damage fit i64")
}
fn cost(generic: i64, green: i64) -> ManaCost {
    ManaCost {
        generic: generic.into(),
        required: BTreeMap::from([(ManaType::Green, green.into())]),
    }
}
fn creature(power: i64, toughness: i64, casting_cost: Option<ManaCost>) -> Program {
    Program::creature(
        casting_cost,
        power.into(),
        toughness.into(),
        std::collections::BTreeSet::new(),
    )
}
fn seed(card: CardId, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card,
        owner,
        destination,
        program,
    }
}
fn resource(owner: SeatId, kind: ManaType, amount: i64) -> SeedCard {
    seed(
        RESOURCE,
        owner,
        ObjectDestination::Battlefield { controller: owner },
        Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(0),
                output: ManaOutput::Fixed(BTreeMap::from([(kind, amount.into())])),
            }],
        ),
    )
}
fn fixture(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    scenario: &'static str,
) -> GameState {
    let (program, green, colourless) = match scenario {
        "creature-resolve" | "creature-counter" => (creature(6, 4, Some(cost(4, 2))), 2, 4),
        "zero-toughness" => (creature(8, 0, Some(cost(2, 1))), 1, 2),
        "artifact-ability" => (
            Program::artifact(
                Some(ManaCost::generic(4.into())),
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(0),
                    mana_cost: ManaCost::generic(3.into()),
                    source_cost: SourceCost::Tap,
                    body: ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::DealDamage(Box::new(
                            spellwake_engine::toolbox::DealDamageSpec {
                                source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                                recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                amount: spellwake_engine::program::DamageAmountSpec::Expr(
                                    spellwake_engine::program::ValueExpr::literal(1),
                                ),
                                exile_if_dies: false,
                            },
                        ))],
                    ),
                }],
            ),
            0,
            4,
        ),
        _ => panic!("unknown authored scenario"),
    };
    let mut cards = vec![seed(SPELL, A, ObjectDestination::Hand, program)];
    if scenario == "artifact-ability" {
        cards.push(seed(
            TARGET,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature(6, 4, None),
        ));
        cards.push(resource(A, ManaType::Colourless, 3));
    }
    if scenario == "creature-counter" {
        cards.push(seed(
            RESPONSE,
            B,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::specific(ManaType::Blue, 2.into()),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                    vec![Instruction::StackOp(Box::new(
                        spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                            spellwake_engine::toolbox::CounterDestination::Graveyard,
                        ),
                    ))],
                ),
            ),
        ));
        cards.push(resource(B, ManaType::Blue, 2));
    }
    let initialized = spellwake_reference_tools::current_capture::initialize_scheduled(
        GameId(41),
        GameContext::new(
            "native-permanent-v1".into(),
            vocabulary.rules_revision().to_owned(),
            "authored".into(),
            "permanent-casting".into(),
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
                [(ManaType::Green, green), (ManaType::Colourless, colourless)]
                    .into_iter()
                    .filter(|(_, amount)| *amount > 0)
                    .map(|(mana_type, amount)| FloatingMana {
                        player: A,
                        mana_type,
                        amount: amount.into(),
                    })
                    .collect(),
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocabulary.clone(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .expect("valid prepared main-phase fixture");
    assert!(initialized.permanent_entry().is_none());
    assert!(initialized.mana_payment().is_none());
    assert!(initialized.mana_production().is_none());
    assert!(initialized.ability_activation().is_none());
    initialized.into_state()
}
fn payment(value: &ManaPaymentRecord) -> Payment {
    Payment {
        source: format!("{:?}", value.source),
        payer: seat(value.payer),
        generic: value.cost.generic.clone(),
        required: amounts(&value.cost.required),
        spend: value
            .spend
            .iter()
            .map(|spend| (symbol(spend.mana_type), spend.amount.clone()))
            .collect(),
    }
}
fn production(value: &ManaProductionRecord) -> Production {
    Production {
        source: format!("{:?}", value.source),
        ability: spellwake_reference_tools::mana_ref::view(value.ability),
        controller: seat(value.controller),
        produced: amounts(&value.produced),
    }
}
fn movement(paths: &mut Vec<String>, before: ObjectZone, after: ObjectZone) {
    if before != after {
        paths.push(format!("{}->{}", zone(before), zone(after)));
    }
}
fn act(
    recorder: &mut Recorder,
    scenario: &'static str,
    state: &GameState,
    action: &'static str,
    actor: SeatId,
    command: GameCommand,
) -> NativeCandidate {
    let saved = state.clone();
    let expects_payment = matches!(
        command,
        GameCommand::CastUntargeted { .. }
            | GameCommand::Cast { .. }
            | GameCommand::ActivateAbility { .. }
    );
    let expects_production = matches!(command, GameCommand::ActivateMana { .. });
    let expects_activation = matches!(command, GameCommand::ActivateAbility { .. });
    let description = format!("{command:?}");
    let candidate = spellwake_reference_tools::current_capture::advance(
        state,
        GameInput {
            basis: state.basis().clone(),
            actor,
            command,
        },
    )
    .expect("authored root action is legal");
    assert_eq!(state, &saved);
    let expects_entry = matches!(action, "C-settled" | "A-settled" | "Z-settled");
    assert_eq!(candidate.permanent_entry().is_some(), expects_entry);
    assert_eq!(candidate.mana_payment().is_some(), expects_payment);
    assert_eq!(candidate.mana_production().is_some(), expects_production);
    assert_eq!(candidate.ability_activation().is_some(), expects_activation);
    assert!(candidate.land_play().is_none());
    assert!(candidate.turn_transitions().is_empty());
    assert!(candidate.shuffle_journal().is_empty());
    if let Some(record) = candidate.mana_payment() {
        assert_eq!(record.payer, actor);
        assert_eq!(
            record.source,
            candidate
                .state()
                .execution()
                .window()
                .entries()
                .last()
                .unwrap()
                .id()
        );
        *recorder.action_counts.entry(scenario).or_default() += 1;
    }
    let path = recorder.spell_zone_paths.entry(scenario).or_default();
    if let Some(entry) = candidate.permanent_entry() {
        let original = state
            .execution()
            .window()
            .entries()
            .iter()
            .find(|item| item.id() == entry.entry)
            .expect("entry fact names the actual incoming stack entry");
        assert_eq!(original.kind(), &StackKind::CardSpell(entry.before.id()));
        assert_eq!(card(state, SPELL), &entry.before);
        assert_eq!(entry.before.zone(), ObjectZone::Stack);
        assert_eq!(entry.after.zone(), ObjectZone::Battlefield);
        assert_eq!(
            entry
                .before
                .card_id()
                .expect("source-authored card-only trace"),
            entry
                .after
                .card_id()
                .expect("source-authored card-only trace")
        );
        assert_eq!(entry.before.owner(), entry.after.owner());
        assert_eq!(entry.after.controller(), Some(original.controller()));
        assert_eq!(entry.after.tapped(), Some(false));
        assert_ne!(entry.before.id(), entry.after.id());
        assert!(candidate.mana_payment().is_none());
        let current = card(candidate.state(), SPELL);
        if scenario == "zero-toughness" {
            assert_eq!(current.zone(), ObjectZone::Graveyard(A));
            assert_ne!(entry.after.id(), current.id());
        } else {
            assert_eq!(current, &entry.after);
        }
        movement(path, entry.before.zone(), entry.after.zone());
        movement(path, entry.after.zone(), current.zone());
        recorder.permanent_entries.push(EntryEvidence {
            capture_point: spellwake_reference_tools::capture_points::token(
                "EntryEvidence actual writer",
                spellwake_reference_tools::current_profile::SnapshotOwner::GameState(
                    candidate.state(),
                ),
                &spellwake_reference_tools::packet::AliasContext::default(),
            )
            .expect("collect actual current fact"),
            scenario,
            action,
            entry: format!("{:?}", entry.entry),
            before: physical(&entry.before),
            after_before_checks: physical(&entry.after),
            current_after_checks: physical(current),
        });
    } else {
        movement(
            path,
            card(state, SPELL).zone(),
            card(candidate.state(), SPELL).zone(),
        );
    }
    if scenario == "creature-counter" {
        movement(
            &mut recorder.response_zone_path,
            card(state, RESPONSE).zone(),
            card(candidate.state(), RESPONSE).zone(),
        );
    }
    recorder.current_facts.push(CurrentFacts {
        capture_point: spellwake_reference_tools::capture_points::token(
            "CurrentFacts actual writer",
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(candidate.state()),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("collect actual current fact"),
        scenario,
        action,
        actor: seat(actor),
        command: description,
        input_revision: state.basis().revision,
        next_revision: candidate.state().basis().revision,
        input_unchanged: state == &saved,
        permanent_entry_present: candidate.permanent_entry().is_some(),
        payment: candidate.mana_payment().map(payment),
        production: candidate.mana_production().map(production),
        activation_present: candidate.ability_activation().is_some(),
        damage_attempts: candidate.damage_journal().attempts.len(),
        positive_damage: candidate.damage_journal().applied.len(),
    });
    candidate
}
fn observe(
    state: &GameState,
    id: &'static str,
    kind: &'static str,
    actor: SeatId,
    protocol: &'static str,
) -> Row {
    let source = card(state, SPELL);
    let creature = state
        .execution()
        .creature(source.id())
        .expect("authored fixture characteristic invariants");
    let optional = |id| {
        state
            .execution()
            .window()
            .cards()
            .current_identity(id)
            .and_then(|id| state.execution().window().cards().get(id))
    };
    let target = optional(TARGET).and_then(|value| {
        state
            .execution()
            .creature(value.id())
            .expect("authored fixture characteristic invariants")
    });
    let position = state.turn().unwrap().position();
    assert_eq!(position.step, Step::PrecombatMain);
    Row {
        capture_point: spellwake_reference_tools::capture_points::token(
            "observe actual scoped row",
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(state),
            &spellwake_reference_tools::packet::AliasContext::default(),
        )
        .expect("collect actual scoped row"),
        id,
        kind,
        turn: position.number,
        step: "MAIN1",
        active: seat(state.execution().window().active_player()),
        actor: seat(actor),
        priority: seat(holder(state)),
        stack_size: state.execution().window().entries().len(),
        spell_zone: zone(source.zone()),
        spell_owner: seat(source.owner()),
        spell_controller: source.controller().map(seat),
        spell_tapped: source.tapped(),
        spell_power: creature.as_ref().map(|value| bounded(value.power.clone())),
        spell_toughness: creature.map(|value| bounded(value.toughness)),
        response_zone: optional(RESPONSE).map(|value| zone(value.zone())),
        target_zone: optional(TARGET).map(|value| zone(value.zone())),
        target_damage: target.as_ref().map(|value| bounded(value.damage.clone())),
        target_toughness: target.map(|value| bounded(value.toughness)),
        mana: pools(state),
        protocol,
    }
}
fn resolve(
    recorder: &mut Recorder,
    scenario: &'static str,
    state: GameState,
    first_action: &'static str,
    final_action: &'static str,
) -> GameState {
    let actor = holder(&state);
    let state = act(
        recorder,
        scenario,
        &state,
        first_action,
        actor,
        GameCommand::Pass,
    )
    .into_state();
    let actor = holder(&state);
    act(
        recorder,
        scenario,
        &state,
        final_action,
        actor,
        GameCommand::Pass,
    )
    .into_state()
}
fn cast(state: &GameState, green: i64, colourless: i64) -> GameCommand {
    GameCommand::CastUntargeted {
        spell: object(state, SPELL),
        payment: Some(ManaPayment {
            from_lots: Default::default(),
            spend: [(ManaType::Green, green), (ManaType::Colourless, colourless)]
                .into_iter()
                .filter(|(_, amount)| *amount > 0)
                .map(|(mana_type, amount)| ManaSpend {
                    mana_type,
                    amount: amount.into(),
                })
                .collect(),
        }),
    }
}
fn add_resource(
    recorder: &mut Recorder,
    scenario: &'static str,
    state: GameState,
    action: &'static str,
    actor: SeatId,
) -> GameState {
    let identity = object(&state, RESOURCE);
    let candidate = act(
        recorder,
        scenario,
        &state,
        action,
        actor,
        GameCommand::ActivateMana {
            source: identity,
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    );
    assert_eq!(object(candidate.state(), RESOURCE), identity);
    assert_eq!(card(candidate.state(), RESOURCE).tapped(), Some(true));
    recorder.native_only_actions.push(NativeOnlyAction {
        capture_point:spellwake_reference_tools::capture_points::token("NativeOnlyAction actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameState(candidate.state()),&spellwake_reference_tools::packet::AliasContext::default()).expect("collect actual current fact"),
        scenario,
        action,
        reason: "Actual authored tap-mana action replaces the reference callback's trusted later pool seed; this extra history is not a common observation.",
        source_after: physical(card(candidate.state(), RESOURCE)),
        mana_after: pools(candidate.state()),
    });
    candidate.into_state()
}
fn scenario(
    vocabulary: &std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    recorder: &mut Recorder,
    name: &'static str,
) -> Vec<Row> {
    let (before, cast_id, settled) = match name {
        "creature-resolve" => ("C-before", "C-cast", "C-settled"),
        "artifact-ability" => ("A-before", "A-cast", "A-settled"),
        "creature-counter" => ("K-before", "K-cast", "K-settled"),
        "zero-toughness" => ("Z-before", "Z-cast", "Z-settled"),
        _ => panic!("unknown authored scenario"),
    };
    let mut state = fixture(vocabulary, name);
    let mut rows = vec![observe(
        &state,
        before,
        "prepared",
        A,
        "actual-prepared-MAIN1-root",
    )];
    let (green, colourless) = match name {
        "zero-toughness" => (1, 2),
        "artifact-ability" => (0, 4),
        _ => (2, 4),
    };
    let command = cast(&state, green, colourless);
    state = act(recorder, name, &state, cast_id, A, command).into_state();
    rows.push(observe(
        &state,
        cast_id,
        "permanent-cast",
        A,
        "actual-root-CastUntargeted",
    ));
    if name == "creature-counter" {
        state = act(recorder, name, &state, "K-A-pass", A, GameCommand::Pass).into_state();
        state = add_resource(recorder, name, state, "K-native-blue-production", B);
        let original = state.execution().window().entries().first().unwrap().id();
        let command = GameCommand::Cast {
            spell: object(&state, RESPONSE),
            target: TargetRef::Stack(original),
        };
        state = act(recorder, name, &state, "K-response", B, command).into_state();
        rows.push(observe(
            &state,
            "K-response",
            "instant-cast",
            B,
            "actual-root-Cast-after-native-only-blue-production",
        ));
        state = resolve(recorder, name, state, "K-B-pass", settled);
    } else {
        state = resolve(recorder, name, state, "permanent-first-pass", settled);
    }
    rows.push(observe(
        &state,
        settled,
        "resolved-boundary",
        A,
        "actual-root-final-pass-resolution-and-checks; actor-is-associated-active-seat",
    ));
    if name == "artifact-ability" {
        state = add_resource(recorder, name, state, "A-native-colourless-production", A);
        let command = GameCommand::ActivateAbility {
            source: object(&state, SPELL),
            ability: ActivatedAbilityId(0),
            target: TargetRef::Object(object(&state, TARGET)),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 3.into(),
                }],
            }),
        };
        state = act(recorder, name, &state, "A-activated", A, command).into_state();
        rows.push(observe(
            &state,
            "A-activated",
            "ability-activation",
            A,
            "actual-root-ActivateAbility-after-native-only-colourless-production",
        ));
        state = resolve(recorder, name, state, "A-ability-first-pass", "A-damage");
        rows.push(observe(
            &state,
            "A-damage",
            "resolved-boundary",
            A,
            "actual-root-final-pass-resolution-and-checks; actor-is-associated-active-seat",
        ));
    }
    // A later accepted input must not repeat an entry or payment from resolution.
    act(
        recorder,
        name,
        &state,
        "later-record-free-pass",
        A,
        GameCommand::Pass,
    );
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
        "permanent",
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    )
    .unwrap_or_else(|error| {
        eprintln!("capture setup failed: {error}");
        std::process::exit(1)
    });
    let mut recorder = Recorder::default();
    let scenarios: BTreeMap<_, _> = [
        "creature-resolve",
        "artifact-ability",
        "creature-counter",
        "zero-toughness",
    ]
    .into_iter()
    .map(|name| (name, scenario(&vocabulary, &mut recorder, name)))
    .collect();
    assert_eq!(scenarios.values().map(Vec::len).sum::<usize>(), 15);
    assert_eq!(recorder.permanent_entries.len(), 3);
    assert_eq!(recorder.native_only_actions.len(), 2);
    let mut observations = serde_json::json!({
        "schema_version": 1,
        "scope": "Actual prepared-MAIN1 GameState root casts, responses, ordinary physical completion, supported state checks and artifact ability; generic authored programs only",
        "scenarios": scenarios,
        "permanent_entries": recorder.permanent_entries,
        "current_facts": recorder.current_facts,
        "native_only_actions": recorder.native_only_actions,
        "spell_zone_paths": recorder.spell_zone_paths,
        "response_zone_path": recorder.response_zone_path,
        "action_counts": recorder.action_counts,
        "limits": [
            "No expected or reference artifact is read by this binary. Common rows come from actual GameState candidates and canonical physical/pool state.",
            "Each profile begins at a separate prepared first-main boundary with already-floating unrestricted initial mana, not a complete opening or turn history.",
            "The two disclosed authored-land mana actions replace reference trusted callback seeds for later resources. They are additional native history and are not common-state history equivalence or real Island/mana-source qualification.",
            "Common resolved-row actor is the associated active seat; current_facts records the actual final-pass actor and exact candidate revisions.",
            "PT and damage fixture observations are bounded integers; every pool/payment quantity retains RulesInteger decimal-string serialization.",
            "Force of Savagery motivates only authored 8/0, 2G entry/death behavior; trample is outside this mechanism. No new registry binding or whole card/game is qualified.",
            "No ETB/replacements, general permanent copies/tokens, public projection, durable replay or measured capacity qualification."
        ]
    });
    spellwake_reference_tools::capture_points::bind_value_points(
        &mut observations,
        "/observations",
    )
    .unwrap_or_else(|error| {
        eprintln!("capture output failed: {error}");
        std::process::exit(1)
    });
    let envelope = spellwake_reference_tools::current_capture::finish("permanent", observations)
        .unwrap_or_else(|error| {
            eprintln!("capture output failed: {error}");
            std::process::exit(1)
        });
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).expect("serialize actual current capture")
    );
}
