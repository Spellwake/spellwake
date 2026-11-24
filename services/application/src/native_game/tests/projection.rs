//! Explicit synthetic display facts over genuine-vocabulary native mechanisms.
//! These descriptions are not source/catalogue/card qualification.
use super::*;
use crate::{amount::Amount, player_wire::*};
use spellwake_engine::{
    characteristics::ObjectCharacteristics,
    object_identity::{CardId, GameObject, ObjectZone},
    occurrence::{ExecutionBindings, WaitingInstance},
    program::{ExecutableBody, Program},
    stack_window::StackEntry,
    token::TokenEntryPreview,
};
use std::{cell::RefCell, io::Write, sync::Mutex};
mod cascade;
mod face_down;
mod madness;
mod mutate;
mod ninja;
mod stored_view;

// Test-only call recorder enum; keeping variants unboxed simplifies test assertions.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, PartialEq, Eq)]
enum DisplayCall {
    Object(GameObject, Program),
    Ability(GameObject, ObjectCharacteristics, NativeAbilityReference),
    Stack(
        StackEntry,
        Option<ExecutableBody>,
        Option<ExecutionBindings>,
    ),
    Token(TokenEntryPreview),
    Trigger(WaitingInstance),
    Prevention(spellwake_engine::effects::DamageAllocationDomain),
}
struct Display {
    context: spellwake_engine::game::GameContext,
    seen: RefCell<Vec<Option<CardId>>>,
    calls: RefCell<Vec<DisplayCall>>,
    invalid_uuid: bool,
    invalid_text: bool,
    fail_labels: bool,
    scope: NativeDisplayScope,
    selected_identity: bool,
}
impl Default for Display {
    fn default() -> Self {
        Self {
            context: context(&vocabulary()),
            seen: RefCell::new(vec![]),
            calls: RefCell::new(vec![]),
            invalid_uuid: false,
            invalid_text: false,
            fail_labels: false,
            scope: NativeDisplayScope::SourceOnly,
            selected_identity: false,
        }
    }
}
impl NativeDisplaySource for Display {
    fn scope(&self) -> NativeDisplayScope {
        self.scope
    }
    fn context(&self) -> &spellwake_engine::game::GameContext {
        &self.context
    }
    fn object(
        &self,
        object: &GameObject,
        definition: &Program,
    ) -> Result<NativeObjectDisplay, NativeProjectionError> {
        self.calls
            .borrow_mut()
            .push(DisplayCall::Object(object.clone(), definition.clone()));
        self.seen.borrow_mut().push(object.card_id());
        Ok(NativeObjectDisplay {
            definition_id: if self.invalid_uuid {
                Some(Uuid::nil())
            } else {
                self.selected_identity.then_some(Uuid::from_u128(17))
            },
            printing_id: None,
            characteristics: PlayerCharacteristics {
                name: if self.invalid_text {
                    "invalid\0display".into()
                } else {
                    "Literal mechanism fixture".into()
                },
                type_line: "Explicit synthetic description".into(),
                rules_text: "Not a qualified card definition".into(),
                mana_cost: None,
                printed_power: None,
                printed_toughness: None,
                printed_loyalty: None,
                printed_defense: None,
            },
        })
    }
    fn ability(
        &self,
        object: &GameObject,
        current: &ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError> {
        self.calls.borrow_mut().push(DisplayCall::Ability(
            object.clone(),
            current.clone(),
            ability,
        ));
        if self.fail_labels {
            Err(NativeProjectionError::MissingDisplay)
        } else {
            Ok("Literal ability description".into())
        }
    }
    fn stack(
        &self,
        entry: &StackEntry,
        body: Option<&ExecutableBody>,
        bindings: Option<&ExecutionBindings>,
    ) -> Result<String, NativeProjectionError> {
        self.calls.borrow_mut().push(DisplayCall::Stack(
            entry.clone(),
            body.cloned(),
            bindings.cloned(),
        ));
        Ok("Literal stack description".into())
    }
    fn token_preview(&self, preview: &TokenEntryPreview) -> Result<String, NativeProjectionError> {
        self.calls
            .borrow_mut()
            .push(DisplayCall::Token(preview.clone()));
        Ok("Literal token preview".into())
    }
    fn trigger(&self, instance: &WaitingInstance) -> Result<String, NativeProjectionError> {
        self.calls
            .borrow_mut()
            .push(DisplayCall::Trigger(instance.clone()));
        Ok("Literal trigger description".into())
    }
    fn prevention_attempt(
        &self,
        attempt: &spellwake_engine::effects::DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError> {
        self.calls
            .borrow_mut()
            .push(DisplayCall::Prevention(attempt.clone()));
        Ok("Literal public damage-source description".into())
    }
}
fn metadata() -> NativeProjectionMetadata {
    NativeProjectionMetadata {
        through_event_sequence: Amount::from(0u64),
        deadline_at: None,
        disclosure: NativeDisclosure::LiveParticipant,
    }
}
#[test]
#[ignore = "requires genuine vocabulary; independent projection metadata, not source/card qualification"]
fn projection_keeps_actual_facts_separate_from_candidate_values() {
    let source = main_state(vec![card(
        1,
        A,
        ObjectDestination::Hand,
        Program::land(BTreeSet::from([LandType::Island]), vec![]),
    )]);
    let mut scope = scope(&source, viewer_a());
    let projected = scope
        .project_with_facts(&metadata(), &Display::default())
        .unwrap();
    let facts = projected
        .facts(source.execution().creature_vocabulary())
        .unwrap();
    projected.snapshot().validate(&facts).unwrap();
    assert!(!facts.authorized_full_disclosure);
    assert!(
        facts
            .known_identities
            .contains(&projected.snapshot().visible_objects[0].handle)
    );
    let mut candidate = projected.snapshot().clone();
    candidate.visible_objects[0].identity = PlayerObjectIdentity::Concealed {};
    assert!(candidate.validate(&facts).is_err());
    let foreign = spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
        "foreign-shape-control".to_owned(),
        [0; 32],
        vec!["Elf".to_owned()],
    )
    .unwrap();
    assert!(matches!(
        projected.facts(&foreign),
        Err(NativeProjectionError::InvalidMetadata)
    ));
}
#[test]
#[ignore = "requires genuine vocabulary; display scope mechanism, not source qualification"]
fn display_physical_identity_is_scoped_to_exact_game_and_context_atomically() {
    let source = state(vec![card(
        1,
        A,
        ObjectDestination::Hand,
        Program::artifact(None, vec![]),
    )]);
    let before = source.clone();
    let mut view = scope(&source, viewer_a());
    let references = view.references.clone();
    let handles = view.used_handles.clone();
    let game_id = view.basis().game_id;
    let context_id = view.basis().context_id;
    for declared in [
        NativeDisplayScope::Game {
            game_id: Uuid::from_u128(game_id.as_u128() ^ 1),
            context_id,
        },
        NativeDisplayScope::Game {
            game_id,
            context_id: Uuid::from_u128(context_id.as_u128() ^ 1),
        },
    ] {
        let foreign = Display {
            scope: declared,
            selected_identity: true,
            ..Display::default()
        };
        assert!(matches!(
            view.project(&metadata(), &foreign),
            Err(NativeProjectionError::InvalidMetadata)
        ));
        assert!(foreign.calls.borrow().is_empty());
        assert!(view.references == references);
        assert!(view.used_handles == handles);
    }
    let unscoped = Display {
        selected_identity: true,
        ..Display::default()
    };
    assert!(matches!(
        view.project(&metadata(), &unscoped),
        Err(NativeProjectionError::InvalidMetadata)
    ));
    assert!(view.references == references);
    assert!(view.used_handles == handles);
    let exact = Display {
        scope: NativeDisplayScope::Game {
            game_id,
            context_id,
        },
        selected_identity: true,
        ..Display::default()
    };
    let snapshot = view.project(&metadata(), &exact).unwrap();
    assert!(snapshot.visible_objects.iter().any(|object| matches!(
        object.identity,
        PlayerObjectIdentity::Known {
            definition_id: Some(id), ..
        } if id == Uuid::from_u128(17)
    )));
    assert_eq!(source, before);
}
#[test]
#[ignore = "requires explicit genuine vocabulary; terminal projection composition only"]
fn conceded_game_projects_root_loser_and_clears_retained_priority() {
    let source = main_state(vec![]);
    let before = source.clone();
    let mut live = scope(&source, viewer_a());
    let live_snapshot = live.project(&metadata(), &Display::default()).unwrap();
    assert_eq!(
        live_snapshot.turn.as_ref().unwrap().priority_seat_ids,
        vec![viewer_a()]
    );
    for viewer in [viewer_a(), viewer_b()] {
        let view = scope(&source, viewer);
        let accepted = view
            .propose(&command(&view, PlayerAction::Concede {}))
            .unwrap();
        let terminal = accepted.candidate().state();
        let mut observer = scope(terminal, viewer);
        let mut completed = metadata();
        completed.disclosure = NativeDisclosure::CompletedParticipant;
        let snapshot = observer.project(&completed, &Display::default()).unwrap();
        assert!(snapshot.turn.as_ref().unwrap().priority_seat_ids.is_empty());
        assert!(snapshot.decisions.is_empty());
        for seat in &snapshot.players {
            assert_eq!(seat.eliminated, seat.seat_id == viewer);
        }
        assert_eq!(source, before);
    }
}
static CAPTURE: Mutex<()> = Mutex::new(());
fn capture(snapshot: &PlayerSnapshot) {
    if let Some(path) = std::env::var_os("SPELLWAKE_NATIVE_VIEW_CAPTURE") {
        let _guard = CAPTURE.lock().unwrap();
        let mut bytes = serde_json::to_vec(snapshot).unwrap();
        bytes.push(b'\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(&bytes).unwrap();
    }
}
pub(super) fn project_test(
    view: &mut NativeScope<'_>,
) -> Result<PlayerSnapshot, NativeProjectionError> {
    let display = Display::default();
    view.project(&metadata(), &display)
}

pub(super) fn verify_view(view: &mut NativeScope<'_>, original: Option<&PlayerCommand>) {
    let source = view.source;
    let before = source.clone();
    let display = Display::default();
    let snapshot = view.project(&metadata(), &display).unwrap();
    assert_display_trace(&snapshot, view, &display);
    assert_eq!(snapshot.basis, *view.basis());
    assert_eq!(snapshot.viewer_seat_id, view.viewer_seat_id());
    if let Some(command) = original
        && matches!(command.action, PlayerAction::Choose { .. })
    {
        assert_eq!(snapshot.decisions.len(), 1);
        snapshot.decisions[0]
            .validate_response(
                command,
                view.viewer_seat_id(),
                &[viewer_a(), viewer_b()],
                &[],
            )
            .unwrap();
    }
    let hidden: BTreeSet<_> = source
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|object| {
            matches!(object.zone(), ObjectZone::Library(_))
                || matches!(object.zone(),ObjectZone::Hand(seat) if seat!=view.binding.actor && !source.execution().revealed_by_ninjutsu(object.id()))
        })
        .map(GameObject::id)
        .collect();
    for (_, reference) in &view.references {
        if let NativeReference::Object(id) = reference {
            assert!(
                !hidden.contains(id),
                "hidden object issued by projected view"
            );
        }
    }
    let hidden_cards: BTreeSet<_> = hidden
        .iter()
        .filter_map(|id| {
            source
                .execution()
                .window()
                .cards()
                .get(*id)
                .and_then(GameObject::card_id)
        })
        .collect();
    assert!(
        display
            .seen
            .borrow()
            .iter()
            .flatten()
            .all(|id| !hidden_cards.contains(id))
    );
    assert_eq!(source, &before);
    capture(&snapshot);
    let observer = if view.viewer_seat_id() == viewer_a() {
        viewer_b()
    } else {
        viewer_a()
    };
    let mut other = scope(source, observer);
    let observer_display = Display::default();
    let observed = other.project(&metadata(), &observer_display).unwrap();
    assert_display_trace(&observed, &other, &observer_display);
    if source
        .pending_decision()
        .is_some_and(|pending| pending.chooser() != other.binding.actor)
    {
        assert!(observed.decisions.is_empty());
    }
    capture(&observed);
}

#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; actual snapshot mechanisms, not card qualification"]
fn projection_visibility_metadata_and_registry_failures_preserve_source() {
    let mut cards = vec![];
    for (id, owner, destination) in [
        (1, A, ObjectDestination::Hand),
        (2, B, ObjectDestination::Hand),
        (3, A, ObjectDestination::LibraryBottom),
        (4, B, ObjectDestination::LibraryBottom),
        (5, A, ObjectDestination::Graveyard),
        (6, B, ObjectDestination::Battlefield { controller: B }),
        (7, A, ObjectDestination::Graveyard),
    ] {
        cards.push(card(
            id,
            owner,
            destination,
            Program::artifact(None, vec![]),
        ));
    }
    let source = state(cards);
    let before = source.clone();
    let mut view = scope(&source, viewer_a());
    let original = view.references.clone();
    let c = &source.basis().context;
    let foreign = Display {
        context: spellwake_engine::game::GameContext::new(
            "foreign-display-release".into(),
            c.rules().into(),
            c.catalogue().into(),
            c.programs().into(),
            c.profile().into(),
            c.creature_vocabulary_id(),
            c.name_rules_id(),
        )
        .unwrap(),
        ..Display::default()
    };
    assert!(matches!(
        view.project(&metadata(), &foreign),
        Err(NativeProjectionError::InvalidMetadata)
    ));
    assert!(view.references == original);
    assert!(foreign.seen.borrow().is_empty());

    let mut invalid = metadata();
    invalid.deadline_at = Some("not-a-time".into());
    assert!(matches!(
        view.project(&invalid, &Display::default()),
        Err(NativeProjectionError::InvalidMetadata)
    ));
    assert!(view.references == original);
    let mut disclosure = metadata();
    disclosure.disclosure = NativeDisclosure::CompletedParticipant;
    assert!(matches!(
        view.project(&disclosure, &Display::default()),
        Err(NativeProjectionError::NotCompleted)
    ));
    assert!(view.references == original);
    let invalid = Display {
        invalid_uuid: true,
        ..Display::default()
    };
    assert!(matches!(
        view.project(&metadata(), &invalid),
        Err(NativeProjectionError::InvalidMetadata)
    ));
    assert!(view.references == original);
    let display = Display::default();
    let snapshot = view.project(&metadata(), &display).unwrap();
    assert_eq!(
        display
            .seen
            .borrow()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            Some(CardId(1)),
            Some(CardId(5)),
            Some(CardId(6)),
            Some(CardId(7))
        ])
    );
    assert_eq!(snapshot.visible_objects.len(), 4);
    assert_eq!(snapshot.zones.len(), 10);
    let grave = snapshot
        .zones
        .iter()
        .find(|zone| {
            zone.zone_type() == PlayerZoneKind::Graveyard
                && zone.owner_seat_id() == Some(viewer_a())
        })
        .unwrap();
    assert!(grave.order_known());
    let ordered = source
        .execution()
        .window()
        .cards()
        .graveyard(A)
        .iter()
        .map(|id| view.issue(NativeReference::Object(*id)).unwrap())
        .collect::<Vec<_>>();
    assert!(grave.objects() == ordered);

    for (kind, owner, hidden) in [
        (PlayerZoneKind::Hand, viewer_a(), false),
        (PlayerZoneKind::Hand, viewer_b(), true),
        (PlayerZoneKind::Library, viewer_a(), true),
        (PlayerZoneKind::Library, viewer_b(), true),
    ] {
        let zone = snapshot
            .zones
            .iter()
            .find(|zone| zone.zone_type() == kind && zone.owner_seat_id() == Some(owner))
            .unwrap();
        assert_eq!(matches!(zone, PlayerZone::Hidden { .. }), hidden);
    }
    assert_eq!(source, before);
    capture(&snapshot);
    // Every new typed zone handle is unusable as a physical card command.
    let zone = snapshot.zones[0].handle().clone();
    assert!(matches!(
        view.propose(&command(&view, PlayerAction::PlayLand { source: zone })),
        Err(NativeBridgeError::WrongReferenceKind)
    ));
}

#[test]
fn projection_error_preserves_private_cause_without_formatting_it() {
    let error = NativeProjectionError::metadata(std::io::Error::other("private provider detail"));
    assert!(!error.to_string().contains("private"));
    assert!(!format!("{error:?}").contains("private"));
    assert_eq!(
        std::error::Error::source(&error).unwrap().to_string(),
        "private provider detail"
    );
}
#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; actual snapshot mechanisms, not card qualification"]
fn projection_exact_resources_turn_terminal_disclosure_and_ability_failure() {
    use spellwake_engine::{
        mana::FloatingMana,
        setup::StartingLife,
        turn::{Step, TurnStart},
    };
    let huge = crate::player_wire::RulesInteger::try_from(
        "184467440737095516170000000000000000000".to_owned(),
    )
    .unwrap();
    let mut input = seed(vec![
        card(
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::creature(None, (-3).into(), huge.clone(), BTreeSet::new()),
        ),
        card(
            2,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
        card(
            3,
            B,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
    ]);
    input.life = vec![
        StartingLife {
            player: A,
            total: huge.clone(),
        },
        StartingLife {
            player: B,
            total: 20.into(),
        },
    ];
    input.mana = vec![FloatingMana {
        player: A,
        mana_type: ManaType::Blue,
        amount: huge.clone(),
    }];
    let source = GameState::initialize_scheduled(
        GameId(90),
        context(&input.creature_vocabulary),
        input,
        TurnStart {
            number: u64::MAX,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let mut view = scope(&source, viewer_a());
    let snapshot = view.project(&metadata(), &Display::default()).unwrap();
    assert_eq!(snapshot.players[0].life, huge);
    assert_eq!(
        snapshot.players[0]
            .mana_pool
            .iter()
            .find(|entry| entry.color == ManaColor::Blue)
            .unwrap()
            .amount
            .as_rules_integer(),
        &huge
    );
    assert_eq!(
        snapshot.turn.as_ref().unwrap().number,
        RulesCount::try_from(u64::MAX.to_string()).unwrap()
    );
    assert_eq!(
        snapshot.turn.as_ref().unwrap().active_seat_ids,
        vec![viewer_a()]
    );
    assert_eq!(
        snapshot.turn.as_ref().unwrap().priority_seat_ids,
        vec![viewer_a()]
    );

    assert_eq!(
        snapshot.visible_objects[0]
            .public_properties
            .as_ref()
            .unwrap()
            .power,
        Some((-3).into())
    );
    capture(&snapshot);
    let live = action_state();
    let mut live_view = scope(&live, viewer_a());
    let prior = live_view
        .issue(NativeReference::Object(object(&live, 3)))
        .unwrap();
    let before = live_view.references.clone();
    assert!(matches!(
        live_view.project(
            &metadata(),
            &Display {
                fail_labels: true,
                ..Display::default()
            }
        ),
        Err(NativeProjectionError::MissingDisplay)
    ));
    assert!(live_view.references == before);
    assert!(
        live_view
            .issue(NativeReference::Object(object(&live, 3)))
            .unwrap()
            == prior
    );
    assert!(matches!(
        live_view.project(
            &metadata(),
            &Display {
                invalid_text: true,
                ..Display::default()
            }
        ),
        Err(NativeProjectionError::InvalidSnapshot)
    ));
    assert!(live_view.references == before);

    let mut input = seed(vec![
        card(
            1,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
        card(
            2,
            B,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
        card(
            3,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
    ]);
    input.life = vec![
        StartingLife {
            player: A,
            total: 0.into(),
        },
        StartingLife {
            player: B,
            total: 20.into(),
        },
    ];
    let terminal = GameState::initialize(GameId(90), context(&input.creature_vocabulary), input)
        .unwrap()
        .into_state();
    assert!(terminal.outcome().is_some());
    let mut view = scope(&terminal, viewer_b());
    let mut full = metadata();
    full.disclosure = NativeDisclosure::CompletedParticipant;
    let snapshot = view.project(&full, &Display::default()).unwrap();
    assert_eq!(snapshot.visible_objects.len(), 3);
    let library = snapshot
        .zones
        .iter()
        .find(|zone| {
            zone.zone_type() == PlayerZoneKind::Library && zone.owner_seat_id() == Some(viewer_a())
        })
        .unwrap();
    assert!(library.order_known());
    let ordered = terminal
        .execution()
        .window()
        .cards()
        .library(A)
        .iter()
        .map(|id| view.issue(NativeReference::Object(*id)).unwrap())
        .collect::<Vec<_>>();
    assert!(library.objects() == ordered);

    assert!(
        snapshot
            .zones
            .iter()
            .all(|zone| matches!(zone, PlayerZone::Visible { .. }))
    );
    assert!(snapshot.players[0].eliminated);
    capture(&snapshot);
}
fn normalize_view(snapshot: &PlayerSnapshot, scope: &NativeScope<'_>) -> serde_json::Value {
    use spellwake_engine::object_identity::ObjectZone;
    use std::collections::BTreeMap;
    let mut names = BTreeMap::new();
    for (handle, reference) in &scope.references {
        let role = match reference {
            NativeReference::Object(id) => format!(
                "visible-card-{}",
                scope
                    .source
                    .execution()
                    .window()
                    .cards()
                    .get(*id)
                    .unwrap()
                    .card_id()
                    .unwrap()
                    .0
            ),
            NativeReference::Zone(zone) => match zone {
                ObjectZone::Library(seat) => format!("library-{}", seat.0),
                ObjectZone::Hand(seat) => format!("hand-{}", seat.0),
                ObjectZone::Graveyard(seat) => format!("graveyard-{}", seat.0),
                ObjectZone::Battlefield => "battlefield".into(),
                ObjectZone::Stack => "stack".into(),
                ObjectZone::Exile => "exile".into(),
                ObjectZone::Command => "command".into(),
            },
            NativeReference::ManaAbility { source, ability } => format!(
                "visible-mana-{}-{ability:?}",
                scope
                    .source
                    .execution()
                    .window()
                    .cards()
                    .get(*source)
                    .unwrap()
                    .card_id()
                    .unwrap()
                    .0
            ),
            _ => panic!("unexpected noninterference fixture reference"),
        };
        names.insert(handle.as_str().to_owned(), role);
    }
    fn rewrite(value: &mut serde_json::Value, names: &BTreeMap<String, String>) {
        match value {
            serde_json::Value::String(text) => {
                if let Some(replacement) = names.get(text) {
                    *text = replacement.clone();
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    rewrite(value, names);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    rewrite(value, names);
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(snapshot).unwrap();
    rewrite(&mut value, &names);
    value["visibleObjects"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|object| object["handle"].as_str().unwrap().to_owned());
    for zone in value["zones"].as_array_mut().unwrap() {
        if zone.get("orderKnown") == Some(&serde_json::Value::Bool(false)) {
            zone["objects"]
                .as_array_mut()
                .unwrap()
                .sort_by_key(|entry| entry.as_str().unwrap().to_owned());
        }
    }
    value
}
#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; actual snapshot mechanisms, not card qualification"]
fn hidden_identity_and_library_order_do_not_change_permitted_projection() {
    fn world(reverse: bool) -> GameState {
        let mut libraries = vec![
            card(
                3,
                A,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ),
            card(
                4,
                A,
                ObjectDestination::LibraryBottom,
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
            ),
        ];
        if reverse {
            libraries.reverse();
        }
        let mut cards = vec![
            card(
                1,
                A,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            ),
            card(
                2,
                B,
                ObjectDestination::Hand,
                if reverse {
                    Program::land(BTreeSet::from([LandType::Island]), vec![])
                } else {
                    Program::artifact(None, vec![])
                },
            ),
            card(
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::land(BTreeSet::from([LandType::Forest]), vec![]),
            ),
        ];
        cards.extend(libraries);
        state(cards)
    }
    let left = world(false);
    let right = world(true);
    let private_order = |source: &GameState| {
        source
            .execution()
            .window()
            .cards()
            .library(A)
            .iter()
            .map(|id| {
                source
                    .execution()
                    .window()
                    .cards()
                    .get(*id)
                    .unwrap()
                    .card_id()
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    assert_ne!(private_order(&left), private_order(&right));
    assert_ne!(
        left.execution().base_program(object(&left, 2)),
        right.execution().base_program(object(&right, 2))
    );
    let mut a = scope(&left, viewer_a());
    let mut b = scope(&right, viewer_a());
    let da = Display::default();
    let db = Display::default();
    let va = a.project(&metadata(), &da).unwrap();
    let vb = b.project(&metadata(), &db).unwrap();
    assert_eq!(normalize_view(&va, &a), normalize_view(&vb, &b));
    assert_eq!(*da.seen.borrow(), *db.seen.borrow());
    capture(&va);
    capture(&vb);
}
#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; actual snapshot mechanisms, not card qualification"]
fn stack_targets_leave_anonymous_occurrence_counts_without_incarnation_rebinding() {
    let initial = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ),
        card(
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
    ]);
    let old = object(&initial, 3);
    let mut current = initial;
    for id in [1, 2] {
        current = bridged(&current, viewer_a(), |view| PlayerAction::CastTargeted {
            source: view
                .issue(NativeReference::Object(object(&current, id)))
                .unwrap(),
            target: PlayerTarget::Object {
                handle: view
                    .issue(NativeReference::Object(object(&current, 3)))
                    .unwrap(),
            },
            payment: None,
        });
    }
    current = bridged(&current, viewer_a(), |_| PlayerAction::Pass {});
    current = bridged(&current, viewer_b(), |_| PlayerAction::Pass {});
    assert_ne!(old, object(&current, 3));
    for viewer in [viewer_a(), viewer_b()] {
        let mut view = scope(&current, viewer);
        let snapshot = view.project(&metadata(), &Display::default()).unwrap();
        assert_eq!(snapshot.stack.len(), 1);
        assert!(snapshot.stack[0].targets.is_empty());
        assert_eq!(
            snapshot.stack[0].unavailable_targets,
            RulesCount::try_from("1".to_owned()).unwrap()
        );
        capture(&snapshot);
    }
    let initial = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::CounterSpell],
            ),
        ),
        card(
            3,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![Instruction::CounterSpell],
            ),
        ),
    ]);
    let mut current = bridged(&initial, viewer_a(), |view| PlayerAction::CastUntargeted {
        source: view
            .issue(NativeReference::Object(object(&initial, 1)))
            .unwrap(),
        payment: None,
    });
    let original = current.execution().window().entries()[0].id();
    for id in [2, 3] {
        current = bridged(&current, viewer_a(), |view| PlayerAction::CastTargeted {
            source: view
                .issue(NativeReference::Object(object(&current, id)))
                .unwrap(),
            target: PlayerTarget::Stack {
                handle: view.issue(NativeReference::Stack(original)).unwrap(),
            },
            payment: None,
        });
    }
    current = bridged(&current, viewer_a(), |_| PlayerAction::Pass {});
    current = bridged(&current, viewer_b(), |_| PlayerAction::Pass {});
    let mut view = scope(&current, viewer_a());
    let snapshot = view.project(&metadata(), &Display::default()).unwrap();
    assert_eq!(snapshot.stack.len(), 1);
    assert!(snapshot.stack[0].targets.is_empty());
    assert_eq!(
        snapshot.stack[0].unavailable_targets,
        RulesCount::try_from("1".to_owned()).unwrap()
    );
    capture(&snapshot);
}

#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; public item boundary only, not capacity qualification"]
fn projected_item_limit_rejects_atomically_and_accepts_the_exact_bound() {
    for (size, valid) in [(MAX_PLAYER_ITEMS, true), (MAX_PLAYER_ITEMS + 1, false)] {
        let cards = (0..size)
            .map(|index| {
                card(
                    u64::try_from(index).unwrap() + 1,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    Program::artifact(None, vec![]),
                )
            })
            .collect();
        let source = state(cards);
        let before = source.clone();
        let mut view = scope(&source, viewer_a());
        let registry = view.references.clone();
        let result = view.project(&metadata(), &Display::default());
        if valid {
            let snapshot = result.unwrap();
            assert_eq!(snapshot.visible_objects.len(), MAX_PLAYER_ITEMS);
            capture(&snapshot);
        } else {
            assert!(matches!(
                result,
                Err(NativeProjectionError::InvalidSnapshot)
            ));
            assert!(view.references == registry);
        }
        assert_eq!(source, before);
    }
}

fn assert_display_trace(snapshot: &PlayerSnapshot, view: &NativeScope<'_>, display: &Display) {
    use spellwake_engine::{effects::DamageDecision, game::RuleFrame};
    let source = view.source;
    let evaluated = source.execution().evaluated_characteristics().unwrap();
    let mut expected = vec![];
    for object in &snapshot.visible_objects {
        let NativeReference::Object(id) = view.reference(&object.handle).unwrap() else {
            panic!("object binding")
        };
        let physical = source.execution().window().cards().get(id).unwrap();
        let current = evaluated.get(id).unwrap();
        expected.push(DisplayCall::Object(
            physical.clone(),
            source.execution().copiable_program(id).unwrap().clone(),
        ));
        for ability in &object.abilities {
            let reference = match view.reference(&ability.handle).unwrap() {
                NativeReference::ManaAbility { source, ability } => {
                    assert_eq!(source, id);
                    NativeAbilityReference::Mana(ability)
                }
                NativeReference::ActivatedAbility { source, ability } => {
                    assert_eq!(source, id);
                    NativeAbilityReference::Activated(ability)
                }
                NativeReference::TriggeredAbility { source, ability } => {
                    assert_eq!(source, id);
                    NativeAbilityReference::Triggered(ability)
                }
                NativeReference::ContinuousAbility { source, ability } => {
                    assert_eq!(source, id);
                    NativeAbilityReference::Continuous(ability)
                }
                _ => panic!("ability binding"),
            };
            expected.push(DisplayCall::Ability(
                physical.clone(),
                current.clone(),
                reference,
            ));
        }
    }
    let public_reveals = source
        .search_reveals()
        .into_iter()
        .chain(source.cascade_reveals())
        .chain(source.face_down_reveals());
    for (object, definition) in public_reveals {
        if view
            .binding
            .seats
            .iter()
            .any(|seat| seat.native_seat == object.owner())
        {
            expected.push(DisplayCall::Object(object.clone(), definition.clone()));
        }
    }
    for entry in source.execution().window().entries() {
        if matches!(
            entry.kind(),
            spellwake_engine::stack_window::StackKind::Speed
                | spellwake_engine::stack_window::StackKind::Monarch
                | spellwake_engine::stack_window::StackKind::DelayedCopy { .. }
                | spellwake_engine::stack_window::StackKind::Emblem { .. }
        ) {
            continue;
        }
        expected.push(DisplayCall::Stack(
            entry.clone(),
            source.execution().stack_body(entry.id()).cloned(),
            source.execution().execution_bindings(entry.id()).cloned(),
        ));
    }
    if let Some(pending) = source
        .pending_decision()
        .filter(|pending| pending.chooser() == view.binding.actor)
    {
        match pending.frame() {
            RuleFrame::TimestampOrder(frame) => {
                for preview in frame.members() {
                    expected.push(DisplayCall::Token(preview.clone()));
                }
            }
            RuleFrame::Triggers(frame) => {
                for id in frame.instances() {
                    if source.execution().waiting_speed_trigger(*id).is_some() {
                        continue;
                    }
                    expected.push(DisplayCall::Trigger(
                        source
                            .execution()
                            .waiting_triggers()
                            .find(|value| value.id == *id)
                            .unwrap()
                            .clone(),
                    ));
                }
            }
            RuleFrame::EntryChoice(frame) => {
                let obj = source
                    .execution()
                    .window()
                    .cards()
                    .get(frame.object())
                    .unwrap();
                let prog = source.execution().copiable_program(frame.object()).unwrap();
                expected.push(DisplayCall::Object(obj.clone(), prog.clone()));
            }
            RuleFrame::Damage(DamageDecision::Allocation(frame)) => {
                for attempt in frame.eligible() {
                    expected.push(DisplayCall::Prevention(attempt.clone()));
                }
            }
            _ => {}
        }
    }
    let calls = display.calls.borrow();
    assert_eq!(calls.len(), expected.len());
    for call in expected {
        assert!(
            calls.contains(&call),
            "description callback changed native provenance"
        );
    }
}

#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; actual current properties and copied-description inputs only"]
fn projected_current_control_counters_damage_tap_and_copy_use_actual_native_owners() {
    use spellwake_contracts::names::{EnglishCardName, ObjectNames};
    use spellwake_engine::characteristics::{
        AbilityChange, Color, ControlRecipient, KeywordAbility, PermanentSelector,
        PowerToughnessCounters, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
    };
    use spellwake_engine::program::{
        CardType, Supertype, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
    };
    let elf = vocabulary().resolve_label("Elf").unwrap();
    let mut target = Program::creature(None, 1.into(), 5.into(), BTreeSet::from([elf]));
    target.printed_keywords.insert(KeywordAbility::Flying);
    target.names = ObjectNames::new(vec![
        EnglishCardName::new("Named resource creature".into()).unwrap(),
        EnglishCardName::new("\u{e000}".into()).unwrap(),
        EnglishCardName::new("\u{10000}".into()).unwrap(),
    ])
    .unwrap();
    target.card_types.insert(CardType::Land);
    target.land_subtypes.insert(LandType::Forest);
    target.supertypes = BTreeSet::from([Supertype::Basic, Supertype::Legendary]);
    target.color_indicator = Some(BTreeSet::from([Color::Blue]));
    target.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(9),
        selector: PermanentSelector::default(),
        parts: StaticParts(vec![StaticPart::Abilities(AbilityChange::grant_keywords(
            [KeywordAbility::Haste, KeywordAbility::Vigilance],
        ))]),
    });
    target.triggered = vec![TriggeredAbility {
        id: TriggeredAbilityId(8),
        condition: spellwake_engine::program::TriggerSpec::becomes_tapped(
            spellwake_engine::program::ObjectFilter::self_object(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    }];
    let copy = ActivatedAbility {
        id: ActivatedAbilityId(7),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::None,
        body: ExecutableBody::new(
            BodyTarget::SinglePermanent,
            vec![Instruction::CopyTargetToSource {
                duration: ResolvedDuration::NoStatedDuration,
                power_toughness: None,
            }],
        ),
    };
    let source = main_state(vec![
        card(
            1,
            B,
            ObjectDestination::Battlefield { controller: B },
            target,
        ),
        card(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(None, vec![copy]),
        ),
        card(
            10,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SinglePermanent,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    duration: ResolvedDuration::NoStatedDuration,
                }],
            ),
        ),
        card(
            20,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: 2.into(),
                        minus_one: 0.into(),
                    },
                }],
            ),
        ),
        card(
            21,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ),
    ]);
    let mut current = bridged(&source, viewer_a(), |view| PlayerAction::CastTargeted {
        source: view
            .issue(NativeReference::Object(object(&source, 10)))
            .unwrap(),
        target: PlayerTarget::Object {
            handle: view
                .issue(NativeReference::Object(object(&source, 1)))
                .unwrap(),
        },
        payment: None,
    });
    current = bridged(&current, viewer_a(), |_| PlayerAction::Pass {});
    current = bridged(&current, viewer_b(), |_| PlayerAction::Pass {});
    current = bridged(&current, viewer_a(), |view| PlayerAction::ActivateMana {
        source: view
            .issue(NativeReference::Object(object(&current, 1)))
            .unwrap(),
        ability: view
            .issue(NativeReference::ManaAbility {
                source: object(&current, 1),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
            })
            .unwrap(),
    });
    for id in [20, 21] {
        current = bridged(&current, viewer_a(), |view| PlayerAction::CastTargeted {
            source: view
                .issue(NativeReference::Object(object(&current, id)))
                .unwrap(),
            target: PlayerTarget::Object {
                handle: view
                    .issue(NativeReference::Object(object(&current, 1)))
                    .unwrap(),
            },
            payment: None,
        });
        current = bridged(&current, viewer_a(), |_| PlayerAction::Pass {});
        current = bridged(&current, viewer_b(), |_| PlayerAction::Pass {});
    }
    let mut view = scope(&current, viewer_a());
    let display = Display::default();
    let snapshot = view.project(&metadata(), &display).unwrap();
    assert_display_trace(&snapshot, &view, &display);
    let native = object(&current, 1);
    let handle = view.issue(NativeReference::Object(native)).unwrap();
    let projected = snapshot
        .visible_objects
        .iter()
        .find(|value| value.handle == handle)
        .unwrap();
    let properties = projected.public_properties.as_ref().unwrap();
    assert_eq!(projected.owner_seat_id, viewer_b());
    assert_eq!(projected.controller_seat_id, Some(viewer_a()));
    assert!(projected.tapped);
    assert_eq!(
        projected.damage_marked,
        RulesCount::try_from("1".to_owned()).unwrap()
    );
    assert_eq!(projected.counters.len(), 1);
    assert_eq!(projected.counters[0].name, "+1/+1");
    assert_eq!(
        projected.counters[0].count,
        RulesCount::try_from("2".to_owned()).unwrap()
    );
    assert_eq!(properties.power, Some(3.into()));
    assert_eq!(properties.toughness, Some(7.into()));
    assert!(properties.card_types == vec![PlayerCardType::Land, PlayerCardType::Creature]);
    assert!(properties.supertypes == vec![PlayerSupertype::Basic, PlayerSupertype::Legendary]);
    assert!(properties.colors == vec![ManaColor::Blue]);
    assert!(properties.land_types == vec![PlayerLandType::Forest]);
    assert_eq!(properties.creature_types[0].as_str(), "Elf");
    assert!(
        properties.keywords
            == vec![
                PlayerKeyword::Haste,
                PlayerKeyword::Vigilance,
                PlayerKeyword::Flying
            ]
    );
    assert!(
        projected
            .abilities
            .iter()
            .any(|ability| ability.kind == PlayerAbilityKind::Mana)
    );
    assert!(
        projected
            .abilities
            .iter()
            .any(|ability| ability.kind == PlayerAbilityKind::Triggered)
    );
    assert!(
        projected
            .abilities
            .iter()
            .any(|ability| ability.kind == PlayerAbilityKind::Continuous)
    );
    let PlayerObjectIdentity::Known { current_names, .. } = &projected.identity else {
        panic!("known identity")
    };
    assert_eq!(
        current_names.iter().next().unwrap().as_str(),
        "Named resource creature"
    );
    assert_eq!(
        current_names
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>(),
        vec!["Named resource creature", "\u{e000}", "\u{10000}"]
    );

    capture(&snapshot);
    // Put the copy ability above the pending public tap trigger, then resolve it.
    current = bridged(&current, viewer_a(), |view| PlayerAction::ActivateAbility {
        source: view
            .issue(NativeReference::Object(object(&current, 2)))
            .unwrap(),
        ability: view
            .issue(NativeReference::ActivatedAbility {
                source: object(&current, 2),
                ability: ActivatedAbilityId(7),
            })
            .unwrap(),
        target: PlayerTarget::Object {
            handle: view
                .issue(NativeReference::Object(object(&current, 1)))
                .unwrap(),
        },
        payment: None,
    });
    current = bridged(&current, viewer_a(), |_| PlayerAction::Pass {});
    current = bridged(&current, viewer_b(), |_| PlayerAction::Pass {});
    let mut view = scope(&current, viewer_a());
    let display = Display::default();
    let snapshot = view.project(&metadata(), &display).unwrap();
    assert_display_trace(&snapshot, &view, &display);
    let handle = view
        .issue(NativeReference::Object(object(&current, 2)))
        .unwrap();
    let copied = snapshot
        .visible_objects
        .iter()
        .find(|value| value.handle == handle)
        .unwrap();
    let properties = copied.public_properties.as_ref().unwrap();
    assert_eq!(copied.owner_seat_id, viewer_a());
    assert_eq!(copied.controller_seat_id, Some(viewer_a()));
    assert!(!copied.tapped);
    assert!(copied.counters.is_empty());
    assert_eq!(
        copied.damage_marked,
        RulesCount::try_from("0".to_owned()).unwrap()
    );
    assert_eq!(properties.power, Some(1.into()));
    assert_eq!(properties.toughness, Some(5.into()));
    assert!(
        current.execution().copiable_program(object(&current, 2))
            != current.execution().base_program(object(&current, 2))
    );
    assert!(
        copied
            .abilities
            .iter()
            .all(|ability| ability.kind != PlayerAbilityKind::Activated)
    );
    capture(&snapshot);
}
#[test]
#[ignore = "requires explicitly supplied genuine vocabulary; typed public-source guard, not an invented damage frame"]
fn captured_damage_sources_keep_public_provenance_after_departure() {
    use crate::native_game::projection::ensure_public_damage_source;
    use spellwake_engine::damage::DamageSource;
    let source = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ),
        card(
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
        card(
            3,
            B,
            ObjectDestination::Hand,
            Program::artifact(None, vec![]),
        ),
        card(
            4,
            A,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ),
    ]);
    let captured = source
        .execution()
        .window()
        .cards()
        .get(object(&source, 2))
        .unwrap()
        .clone();
    let public = DamageSource::Object {
        object: captured.clone(),
        controller: B,
    };
    assert!(ensure_public_damage_source(&public, A).is_ok());
    for id in [3, 4] {
        let private = DamageSource::Object {
            object: source
                .execution()
                .window()
                .cards()
                .get(object(&source, id))
                .unwrap()
                .clone(),
            controller: B,
        };
        assert!(matches!(
            ensure_public_damage_source(&private, A),
            Err(NativeProjectionError::HiddenReference)
        ));
    }
    let cast = bridged(&source, viewer_a(), |view| PlayerAction::CastTargeted {
        source: view
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        target: PlayerTarget::Object {
            handle: view
                .issue(NativeReference::Object(object(&source, 2)))
                .unwrap(),
        },
        payment: None,
    });
    let passed = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let departed = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    assert!(
        departed
            .execution()
            .window()
            .cards()
            .get(captured.id())
            .is_none()
    );
    assert_ne!(object(&departed, 2), captured.id());
    assert!(ensure_public_damage_source(&public, A).is_ok());
    // A native StackEntryId is obtained from an actual copy, never from a scalar.
    let source = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
                vec![Instruction::CopySpell],
            ),
        ),
    ]);
    let cast = bridged(&source, viewer_a(), |view| PlayerAction::CastUntargeted {
        source: view
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let original = cast.execution().window().entries()[0].id();
    let copied = bridged(&cast, viewer_a(), |view| PlayerAction::CastTargeted {
        source: view
            .issue(NativeReference::Object(object(&cast, 2)))
            .unwrap(),
        target: PlayerTarget::Stack {
            handle: view.issue(NativeReference::Stack(original)).unwrap(),
        },
        payment: None,
    });
    let passed = bridged(&copied, viewer_a(), |_| PlayerAction::Pass {});
    let pending = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    assert!(
        pending.pending_decision().is_none(),
        "targetless copy retains without a chooser"
    );
    let retained = pending;
    let copy = *retained
        .execution()
        .window()
        .spell_copies()
        .keys()
        .next()
        .unwrap();
    let provenance = DamageSource::SpellCopy {
        entry: copy,
        controller: A,
    };
    assert!(ensure_public_damage_source(&provenance, B).is_ok());
    // This tests the typed provenance gate, not a fabricated Allocation frame:
    // current singleton copied-spell damage cannot issue a multi-attempt domain.
}

#[test]
#[ignore = "requires source-qualified creature vocabulary"]
fn typecycling_private_search_and_public_reveal_replay_through_player_inputs() {
    use spellwake_engine::program::LibrarySearch;
    let mut cycler = Program::artifact(Some(ManaCost::free()), vec![]);
    cycler.activated.push(ActivatedAbility::typecycling(
        ActivatedAbilityId(6),
        ManaCost::free(),
        LibrarySearch::land_subtype_to_hand(LandType::Island),
    ));
    let source = main_state(vec![
        card(1, A, ObjectDestination::Hand, cycler),
        card(
            2,
            A,
            ObjectDestination::LibraryBottom,
            Program::land(BTreeSet::from([LandType::Island]), vec![]),
        ),
        card(
            3,
            A,
            ObjectDestination::LibraryBottom,
            Program::land(BTreeSet::from([LandType::Forest]), vec![]),
        ),
        card(
            4,
            B,
            ObjectDestination::LibraryBottom,
            Program::land(BTreeSet::from([LandType::Island]), vec![]),
        ),
    ]);
    let mut caller = scope(&source, viewer_a());
    let snapshot = caller.project(&metadata(), &Display::default()).unwrap();
    let hand = &snapshot.visible_objects[0];
    let ability = hand
        .abilities
        .iter()
        .find(|a| a.kind == PlayerAbilityKind::Cycling)
        .expect("hand ability visible");
    let original = command(
        &caller,
        PlayerAction::Cycle {
            source: hand.handle.clone(),
            ability: ability.handle.clone(),
            payment: None,
        },
    );
    let paid = caller.propose(&original).unwrap();
    super::input_codec::verify(&caller, &paid);
    let first = bridged(paid.candidate().state(), viewer_a(), |_| {
        PlayerAction::Pass {}
    });
    let pending = bridged(&first, viewer_b(), |_| PlayerAction::Pass {});
    let mut chooser = scope(&pending, viewer_a());
    let display = Display::default();
    let searching = chooser.project(&metadata(), &display).unwrap();
    let [decision] = searching.decisions.as_slice() else {
        panic!("expected one private search");
    };
    let PlayerDomain::LibrarySearch {
        inspected,
        eligible,
        ..
    } = &decision.domain
    else {
        panic!("expected search contents")
    };
    assert_eq!(inspected.len(), 2);
    assert_eq!(eligible.len(), 1);
    assert!(display.seen.borrow().contains(&Some(CardId(2))));
    assert!(display.seen.borrow().contains(&Some(CardId(3))));
    assert!(!display.seen.borrow().contains(&Some(CardId(4))));
    let mut observer = scope(&pending, viewer_b());
    let display = Display::default();
    assert!(
        observer
            .project(&metadata(), &display)
            .unwrap()
            .decisions
            .is_empty()
    );
    assert!(!display.seen.borrow().contains(&Some(CardId(2))));
    assert!(!display.seen.borrow().contains(&Some(CardId(3))));
    let choice = command(
        &chooser,
        PlayerAction::Choose {
            decision: decision.decision.clone(),
            response: PlayerChoice::Objects {
                objects: vec![eligible[0].handle.clone()],
            },
        },
    );
    decision
        .validate_response(&choice, viewer_a(), &[viewer_a(), viewer_b()], &[])
        .unwrap();
    let found = chooser.propose(&choice).unwrap();
    super::input_codec::verify(&chooser, &found);
    for viewer in [viewer_a(), viewer_b()] {
        let display = Display::default();
        let seen = scope(found.candidate().state(), viewer)
            .project(&metadata(), &display)
            .unwrap();
        let seat = seen
            .players
            .iter()
            .find(|p| p.seat_id == viewer_a())
            .unwrap();
        assert!(seat.search_reveal.is_some());
        assert!(display.seen.borrow().contains(&Some(CardId(2))));
        assert!(!display.seen.borrow().contains(&Some(CardId(3))));
        assert!(!display.seen.borrow().contains(&Some(CardId(4))));
        if viewer == viewer_b() {
            assert_eq!(seen.visible_objects.len(), 1); // Only the discarded cycler is public.
        }
    }
}

#[test]
#[ignore = "requires source-qualified creature vocabulary"]
fn cast_escape_from_graveyard_replays_the_alternative_and_exact_object_payment() {
    use spellwake_engine::program::ConditionalCastCost;
    let mut spell = instant(
        BodyTarget::None,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(2.into()),
        }],
    );
    spell.conditional_cost = Some(ConditionalCastCost::Escape {
        mana: ManaCost::free(),
        exile: 2,
        entry_counters: 0,
    });
    let resource = Program::artifact(None, vec![]);
    let state = main_state(vec![
        card(1, A, ObjectDestination::Graveyard, spell),
        card(2, A, ObjectDestination::Graveyard, resource.clone()),
        card(3, A, ObjectDestination::Graveyard, resource),
    ]);
    let mut view = scope(&state, viewer_a());
    let source = view
        .issue(NativeReference::Object(object(&state, 1)))
        .unwrap();
    let resources = [2, 3]
        .into_iter()
        .map(|n| PlayerCastResource::Escape {
            source: view
                .issue(NativeReference::Object(object(&state, n)))
                .unwrap(),
        })
        .collect();
    let request = command(
        &view,
        PlayerAction::CastWithResources {
            face: None,
            adventure: false,
            hybrid_choices: Vec::new(),
            fixed_targets: vec![],
            chosen_x: None,
            permanent_targets: vec![],
            target_division: vec![],
            modes: vec![],
            source,
            target: None,
            payment: None,
            use_alternative: true,
            resources,
            additional: vec![],
        },
    );
    let proposed = view.propose(&request).unwrap();
    super::input_codec::verify(&view, &proposed);
    let cast = proposed.candidate().state();
    for n in [2, 3] {
        assert_eq!(
            cast.execution()
                .window()
                .cards()
                .get(object(cast, n))
                .unwrap()
                .zone(),
            ObjectZone::Exile
        );
    }
    let step = bridged(cast, viewer_a(), |_| PlayerAction::Pass {});
    let done = bridged(&step, viewer_b(), |_| PlayerAction::Pass {});
    assert_eq!(done.execution().life_total(A), Some(&22.into()));
    assert_eq!(
        done.execution()
            .window()
            .cards()
            .get(object(&done, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
}

#[test]
#[ignore = "requires source-qualified creature vocabulary"]
fn suspend_projects_only_legal_targets_and_replays_the_cast_during_resolution() {
    check_triggered_exile_cast(false);
}

#[test]
#[ignore = "requires source-qualified creature vocabulary"]
fn rebound_projects_its_own_targeted_cast_and_replays_both_resolutions() {
    check_triggered_exile_cast(true);
}
fn check_triggered_exile_cast(rebound: bool) {
    use spellwake_engine::{
        combat::CombatDecision,
        game::{DecisionReply, RuleFrame},
        priority::PriorityPhase,
        program::ExileAction,
        stack_window::WindowPhase,
    };
    let mut spell = instant(
        BodyTarget::SinglePermanent,
        vec![Instruction::ReturnToOwnersHand],
    );
    if rebound {
        spell
            .printed_keywords
            .insert(spellwake_engine::characteristics::KeywordAbility::Rebound);
    } else {
        spell.exile_action = Some(ExileAction::Suspend {
            counters: 1,
            price: ManaCost::free(),
        });
    }
    let thing = Program::artifact(None, vec![]);
    let mut cards = vec![
        card(1, A, ObjectDestination::Hand, spell),
        card(
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
            thing.clone(),
        ),
        card(3, B, ObjectDestination::LibraryBottom, thing.clone()),
        card(4, A, ObjectDestination::LibraryBottom, thing.clone()),
    ];
    if rebound {
        cards.push(card(
            5,
            B,
            ObjectDestination::Battlefield { controller: B },
            thing,
        ));
    }
    let base = main_state(cards);
    let mut state = if rebound {
        let cast = bridged(&base, viewer_a(), |scope| PlayerAction::CastTargeted {
            source: scope
                .issue(NativeReference::Object(object(&base, 1)))
                .unwrap(),
            target: PlayerTarget::Object {
                handle: scope
                    .issue(NativeReference::Object(object(&base, 5)))
                    .unwrap(),
            },
            payment: None,
        });
        let passed = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
        bridged(&passed, viewer_b(), |_| PlayerAction::Pass {})
    } else {
        bridged(&base, viewer_a(), |scope| PlayerAction::Suspend {
            source: scope
                .issue(NativeReference::Object(object(&base, 1)))
                .unwrap(),
            payment: None,
        })
    };
    let exiled = object(&state, 1);
    assert_eq!(
        state.execution().time_counters().get(&exiled).cloned(),
        (!rebound).then(|| 1.into())
    );
    for _ in 0..100 {
        let (actor, command) = if let Some(decision) = state.pending_decision() {
            let choice = match decision.frame() {
                RuleFrame::OptionalEffect(_) => break,
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected {other:?}"),
            };
            (
                decision.chooser(),
                GameCommand::Reply {
                    decision: decision.id(),
                    choice,
                },
            )
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                panic!("expected priority");
            };
            (holder, GameCommand::Pass)
        };
        state = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            })
            .unwrap()
            .into_state();
    }
    assert!(
        scope(&state, viewer_b())
            .project(&metadata(), &Display::default())
            .unwrap()
            .decisions
            .is_empty()
    );
    let cast = bridged(&state, viewer_a(), |access| {
        let snap = access.project(&metadata(), &Display::default()).unwrap();
        let decision = &snap.decisions[0];
        let (spell, target_required, targets) = match &decision.domain {
            PlayerDomain::SuspendedCast {
                allows_x: _,
                spell,
                target_required,
                targets,
            } if !rebound => (spell, target_required, targets),
            PlayerDomain::ReboundCast {
                allows_x: _,
                spell,
                target_required,
                targets,
            } if rebound => (spell, target_required, targets),
            _ => panic!("expected exile cast opportunity"),
        };
        assert!(spell == &access.issue(NativeReference::Object(exiled)).unwrap());
        assert!(target_required);
        let target = PlayerTarget::Object {
            handle: access
                .issue(NativeReference::Object(object(&state, 2)))
                .unwrap(),
        };
        assert!(targets.as_slice() == std::slice::from_ref(&target));
        PlayerAction::Choose {
            decision: decision.decision.clone(),
            response: if rebound {
                PlayerChoice::ReboundCast {
                    face: None,
                    adventure: false,
                    chosen_x: None,
                    target: Some(target),
                    payment: None,
                    resources: vec![],
                    additional: vec![],
                }
            } else {
                PlayerChoice::SuspendCast {
                    face: None,
                    adventure: false,
                    chosen_x: None,
                    target: Some(target),
                    payment: None,
                    resources: vec![],
                    additional: vec![],
                }
            },
        }
    });
    let a_pass = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let done = bridged(&a_pass, viewer_b(), |_| PlayerAction::Pass {});
    for (id, zone) in [(1, ObjectZone::Graveyard(A)), (2, ObjectZone::Hand(B))] {
        assert_eq!(
            done.execution()
                .window()
                .cards()
                .get(object(&done, id))
                .unwrap()
                .zone(),
            zone
        );
    }
}

#[test]
#[ignore = "requires retained source-qualified vocabulary"]
fn cast_bestowed_replays_its_target_and_projects_the_aura_to_both_seats() {
    use spellwake_engine::program::{CardType, ConditionalCastCost};
    let mut bestow = Program::creature(Some(ManaCost::free()), 2.into(), 3.into(), BTreeSet::new());
    bestow.card_types.insert(CardType::Enchantment);
    bestow.conditional_cost = Some(ConditionalCastCost::Bestow(ManaCost::free()));
    let host = Program::creature(None, 3.into(), 3.into(), BTreeSet::new());
    let state = main_state(vec![
        card(1, A, ObjectDestination::Hand, bestow),
        card(2, B, ObjectDestination::Battlefield { controller: B }, host),
    ]);
    let cast = bridged(&state, viewer_a(), |view| PlayerAction::CastWithResources {
        face: None,
        adventure: false,
        hybrid_choices: Vec::new(),
        fixed_targets: vec![],
        chosen_x: None,
        permanent_targets: vec![],
        target_division: vec![],
        modes: vec![],
        source: view
            .issue(NativeReference::Object(object(&state, 1)))
            .unwrap(),
        target: Some(PlayerTarget::Object {
            handle: view
                .issue(NativeReference::Object(object(&state, 2)))
                .unwrap(),
        }),
        use_alternative: true,
        payment: None,
        additional: vec![],
        resources: vec![],
    });
    let first = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let done = bridged(&first, viewer_b(), |_| PlayerAction::Pass {});
    for viewer in [viewer_a(), viewer_b()] {
        for (state, attached) in [(&cast, false), (&done, true)] {
            let seen = scope(state, viewer)
                .project(&metadata(), &Display::default())
                .unwrap();
            let aura = seen
                .visible_objects
                .iter()
                .find(|object| object.bestowed)
                .unwrap();
            assert_eq!(aura.attached_to.is_some(), attached);
            assert_eq!(
                seen.visible_objects.iter().filter(|o| o.bestowed).count(),
                1
            );
        }
    }
}

#[test]
#[ignore = "requires explicitly supplied genuine vocabulary"]
fn both_seats_see_player_speed_and_its_sourceless_stack_ability() {
    use spellwake_engine::characteristics::KeywordAbility;
    let mut engines = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    engines
        .printed_keywords
        .insert(KeywordAbility::StartYourEngines);
    let start = main_state(vec![
        card(
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
            engines,
        ),
        card(
            2,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SinglePlayer,
                vec![Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Lose(1.into()),
                }],
            ),
        ),
    ]);
    let cast = bridged(&start, viewer_a(), |view| PlayerAction::CastTargeted {
        source: view
            .issue(NativeReference::Object(object(&start, 2)))
            .unwrap(),
        target: PlayerTarget::Seat {
            seat_id: viewer_b(),
        },
        payment: None,
    });
    let resolve = |state: &GameState| {
        let first = bridged(state, viewer_a(), |_| PlayerAction::Pass {});
        bridged(&first, viewer_b(), |_| PlayerAction::Pass {})
    };
    let trigger = resolve(&cast);
    let done = resolve(&trigger);
    for viewer in [viewer_a(), viewer_b()] {
        for (state, speed) in [(&trigger, 1), (&done, 2)] {
            let seen = scope(state, viewer)
                .project(&metadata(), &Display::default())
                .unwrap();
            assert_eq!(
                seen.players
                    .iter()
                    .find(|s| s.seat_id == viewer_a())
                    .unwrap()
                    .speed,
                speed
            );
            assert_eq!(
                seen.players
                    .iter()
                    .find(|s| s.seat_id == viewer_b())
                    .unwrap()
                    .speed,
                0
            );
            if speed == 1 {
                let [entry] = seen.stack.as_slice() else {
                    panic!("one inherent trigger");
                };
                assert_eq!(entry.label, "Increase speed");
                assert!(entry.source_object.is_none());
            } else {
                assert!(seen.stack.is_empty());
            }
        }
    }
}
