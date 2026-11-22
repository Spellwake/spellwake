//! Both seats project the same accepted native command through their own handle scope.
use super::*;
use spellwake_engine::{
    face_down::FaceUpAbility,
    mana::FloatingMana,
    program::ExileAction,
    turn::{Step, TurnStart},
};

fn setup_hidden() -> GameState {
    let mut printed = Program::creature(
        Some(ManaCost::generic(4.into())),
        5.into(),
        4.into(),
        BTreeSet::new(),
    );
    printed
        .face_up_abilities
        .push(FaceUpAbility::Morph(ManaCost::generic(0.into())));
    let mut second = printed.clone();
    second.exile_action = Some(ExileAction::Foretell(ManaCost::generic(1.into())));
    let mut setup = seed(vec![
        card(1, A, ObjectDestination::Hand, printed),
        card(2, A, ObjectDestination::Hand, second),
    ]);
    setup.mana.push(FloatingMana {
        player: A,
        mana_type: ManaType::Colourless,
        amount: 5.into(),
    });
    GameState::initialize_scheduled(
        GameId(90),
        context(&setup.creature_vocabulary),
        setup,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}

fn spend(n: i64) -> Option<PlayerPayment> {
    Some(PlayerPayment {
        from_lots: Default::default(),
        exile_cast_type: None,
        alternative_cost_source: None,
        graveyard_permission: None,
        use_granted_flashback: false,
        restricted_mana: Vec::new(),
        spend: if n == 0 {
            vec![]
        } else {
            vec![ManaSpend {
                color: ManaColor::Colourless,
                amount: RulesCount::try_from(n.to_string()).unwrap(),
            }]
        },
    })
}

fn act(
    state: &GameState,
    viewer: Uuid,
    action: impl FnOnce(&mut NativeScope<'_>) -> PlayerAction,
) -> GameState {
    let mut access = scope(state, viewer);
    let action = action(&mut access);
    let original = command(&access, action);
    let proposed = access.propose(&original).unwrap();
    super::super::input_codec::verify(&access, &proposed);
    proposed.into_parts().3.into_state()
}

fn view(state: &GameState, viewer: Uuid) -> (PlayerSnapshot, Display) {
    let display = Display::default();
    let snapshot = scope(state, viewer).project(&metadata(), &display).unwrap();
    (snapshot, display)
}

#[test]
#[ignore = "requires retained vocabulary for the two-seat native projection"]
fn morph_and_foretell_disclose_only_authorized_faces_through_resolution_and_concession() {
    let mut game = setup_hidden();
    game = act(&game, viewer_a(), |v| PlayerAction::Foretell {
        source: v.issue(NativeReference::Object(object(&game, 2))).unwrap(),
        payment: spend(2),
    });
    game = act(&game, viewer_a(), |v| PlayerAction::CastFaceDown {
        source: v.issue(NativeReference::Object(object(&game, 1))).unwrap(),
        appearance: PlayerFaceDownKind::Morph,
        payment: spend(3),
    });
    let (other, display) = view(&game, viewer_b());
    assert_eq!(other.stack[0].label, "Face-down creature spell");
    assert!(display.calls.borrow().is_empty());
    assert_eq!(other.visible_objects.len(), 2);
    for obj in &other.visible_objects {
        assert!(matches!(obj.identity, PlayerObjectIdentity::Concealed {}));
        assert!(obj.face_up_options.is_empty());
        assert!(obj.face_down_order.is_some());
        if obj.face_down.is_some() {
            let properties = obj.public_properties.as_ref().unwrap();
            assert_eq!(properties.power, Some(2.into()));
            assert!(properties.colors.is_empty());
        } else {
            assert!(obj.foretold_on_turn.is_some());
            assert!(obj.public_properties.is_none());
        }
    }
    let (controller, _) = view(&game, viewer_a());
    assert!(
        controller
            .visible_objects
            .iter()
            .all(|obj| matches!(obj.identity, PlayerObjectIdentity::Known { .. }))
    );
    game = act(&game, viewer_a(), |_| PlayerAction::Pass {});
    game = act(&game, viewer_b(), |_| PlayerAction::Pass {});
    let (controller, _) = view(&game, viewer_a());
    let face = controller
        .visible_objects
        .iter()
        .find(|o| o.face_down.is_some())
        .unwrap();
    assert_eq!(face.face_up_options.len(), 1);
    assert_eq!(face.face_up_options[0].ability, 0);
    let still_hidden = game.clone();
    game = act(&game, viewer_a(), |v| PlayerAction::TurnFaceUp {
        source: v.issue(NativeReference::Object(object(&game, 1))).unwrap(),
        ability: 0,
        payment: spend(0),
    });
    let (other, display) = view(&game, viewer_b());
    assert!(display.seen.borrow().contains(&Some(CardId(1))));
    assert!(!display.seen.borrow().contains(&Some(CardId(2))));
    assert!(other.visible_objects.iter().any(|obj| {
        obj.public_properties
            .as_ref()
            .is_some_and(|props| props.power == Some(5.into()))
    }));
    let ended = act(&still_hidden, viewer_b(), |_| PlayerAction::Concede {});
    let (other, display) = view(&ended, viewer_b());
    assert!(
        display.seen.borrow().contains(&Some(CardId(1)))
            && display.seen.borrow().contains(&Some(CardId(2)))
    );
    assert_eq!(
        other
            .players
            .iter()
            .find(|p| p.seat_id == viewer_a())
            .unwrap()
            .face_down_reveals
            .len(),
        2
    );
}
