use super::*;
use spellwake_engine::{
    characteristics::{ControlRecipient, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec},
    game::RuleFrame,
    land::BasicLandType,
    mana::{ManaAbilityId, ManaAbilityRef, ManaCost},
};

const GARRISON: u64 = 1;
const FOREST: u64 = 2;
const CITADEL: u64 = 3;
const BEARS: u64 = 4;
const OGRE: u64 = 5;
const VINDICATE: u64 = 6;
const UNSUMMON: u64 = 7;
const BOOMERANG: u64 = 8;
const RETURN_CONTROL: u64 = 9;
const OPPONENT_LAND: u64 = 10;

fn cast_garrison(extra: impl IntoIterator<Item = SeedCard>) -> GameState {
    use ManaType::{Black, Blue, Colourless, White};
    use ObjectDestination::{Battlefield, Hand, LibraryBottom};
    let start = game(
        [
            seed("Darksteel Garrison", GARRISON, A, Hand),
            seed("Forest", FOREST, B, Battlefield { controller: A }),
            seed(
                "Darksteel Citadel",
                CITADEL,
                A,
                Battlefield { controller: A },
            ),
            seed("Grizzly Bears", BEARS, B, Battlefield { controller: B }),
            seed("Gray Ogre", OGRE, B, Battlefield { controller: B }),
            seed("Vindicate", VINDICATE, A, Hand),
            seed("Unsummon", UNSUMMON, B, Hand),
            seed("Boomerang", BOOMERANG, B, Hand),
            SeedCard {
                card: CardId(RETURN_CONTROL),
                owner: B,
                destination: Hand,
                program: Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::SINGLE_PERMANENT,
                        vec![
                            crate::keyword_creatures::toolbox_emit::apply_continuous_parts(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                                    ControlRecipient::TargetOwner,
                                )]),
                                ResolvedDuration::NoStatedDuration,
                            ),
                        ],
                    ),
                ),
            },
            seed("Island", OPPONENT_LAND, B, Battlefield { controller: B }),
            seed("Forest", 11, B, LibraryBottom),
        ]
        .into_iter()
        .chain(extra)
        .collect(),
        &[
            (A, Colourless, 12),
            (A, White, 1),
            (A, Black, 1),
            (B, Blue, 2),
        ],
    );
    let cast = advance(
        &start,
        A,
        GameCommand::CastUntargeted {
            spell: object(&start, GARRISON),
            payment: Some(payment(&[(Colourless, 2)])),
        },
    );
    assert_eq!(cast.execution().mana(A, Colourless), 10.into());
    let entered = resolve_top(&cast);
    assert_eq!(
        entered.permanent_entry().unwrap().after.id(),
        object(entered.state(), GARRISON)
    );
    let state = entered.into_state();
    assert_eq!(attachment(&state), None);
    assert_eq!(physical(&state, FOREST).owner(), B);
    assert_eq!(physical(&state, FOREST).controller(), Some(A));
    state
}

fn attachment(state: &GameState) -> Option<TargetRef> {
    state.execution().attached_to(object(state, GARRISON))
}

fn indestructible(state: &GameState, card: u64) -> bool {
    state
        .execution()
        .current_characteristics(object(state, card))
        .unwrap()
        .unwrap()
        .has_keyword(KeywordAbility::Indestructible)
}

fn fortify(state: &GameState, target: u64) -> GameCommand {
    GameCommand::ActivateAbility {
        source: object(state, GARRISON),
        ability: ActivatedAbilityId(1),
        target: TargetRef::Object(object(state, target)),
        payment: Some(payment(&[(ManaType::Colourless, 3)])),
    }
}

fn attach(state: &GameState, card: u64) -> GameState {
    let target = object(state, card);
    let paying = advance(state, A, fortify(state, card));
    assert_eq!(
        paying.execution().mana(A, ManaType::Colourless),
        state
            .execution()
            .mana(A, ManaType::Colourless)
            .minus(&3.into()),
    );
    let resolved = resolve_top(&paying).into_state();
    assert_eq!(attachment(&resolved), Some(TargetRef::Object(target)));
    resolved
}

fn tap_and_target(
    state: &GameState,
    land: u64,
    ability: ManaAbilityRef,
    mana: ManaType,
    target: u64,
) -> GameState {
    let before = state.execution().mana(A, mana);
    let tapped = advance(
        state,
        A,
        GameCommand::ActivateMana {
            source: object(state, land),
            ability,
        },
    );
    assert_eq!(physical(&tapped, land).tapped(), Some(true));
    assert_eq!(tapped.execution().mana(A, mana), before.plus(&1.into()));
    let decision = tapped.pending_decision().unwrap();
    let RuleFrame::TriggerTarget(frame) = decision.frame() else {
        panic!("expected Garrison's targeted tap trigger, got {decision:?}");
    };
    assert_eq!(
        frame.domain(),
        &[
            TargetRef::Object(object(&tapped, BEARS)),
            TargetRef::Object(object(&tapped, OGRE)),
        ]
    );
    let target = TargetRef::Object(object(&tapped, target));
    let entry = frame.entry();
    let stacked = reply(&tapped, A, DecisionReply::TriggerTarget(target));
    let [trigger] = stacked.execution().window().entries() else {
        panic!("expected exactly one tap trigger");
    };
    assert_eq!(trigger.id(), entry);
    assert_eq!(trigger.controller(), A);
    assert_eq!(trigger.targets().targets()[0].choice().target, target);
    stacked
}

fn assert_creature_size(state: &GameState, card: u64, size: i64) {
    let pt = state
        .execution()
        .power_toughness(object(state, card))
        .unwrap()
        .unwrap()
        .derived;
    assert_eq!((pt.power, pt.toughness), (size.into(), size.into()));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn darksteel_garrison_protects_the_fortified_forest_and_its_tap_pump_expires_at_cleanup() {
    use ManaType::{Black, Blue, Colourless, Green, White};
    let cast = cast_garrison([]);
    assert!(!indestructible(&cast, FOREST));
    let attached = attach(&cast, FOREST);
    assert!(indestructible(&attached, FOREST));
    let forest = object(&attached, FOREST);
    let destroy = advance(
        &attached,
        A,
        targeted_spell(
            &attached,
            VINDICATE,
            forest,
            &[(Colourless, 1), (White, 1), (Black, 1)],
        ),
    );
    let destroyed = resolve_top(&destroy).into_state();
    assert_eq!(
        physical(&destroyed, VINDICATE).zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(physical(&destroyed, FOREST).id(), forest);
    assert_eq!(physical(&destroyed, FOREST).zone(), ObjectZone::Battlefield);
    assert_eq!(attachment(&destroyed), Some(TargetRef::Object(forest)));

    let stacked = tap_and_target(
        &destroyed,
        FOREST,
        ManaAbilityRef::BasicLand(BasicLandType::Forest),
        Green,
        BEARS,
    );
    let tap_trigger = stacked.execution().window().entries()[0].id();
    assert_creature_size(&stacked, BEARS, 2);
    let pumped = resolve_top(&stacked).into_state();
    assert_creature_size(&pumped, BEARS, 3);
    assert_creature_size(&pumped, OGRE, 2);
    let next_turn = exile_hand::until(pumped, |game| game.turn().unwrap().position().number == 2);
    assert_creature_size(&next_turn, BEARS, 2);

    // A separate continuation removes the target in the response window.
    let bears = object(&stacked, BEARS);
    let passed = advance(&stacked, A, GameCommand::Pass);
    let bounce = advance(
        &passed,
        B,
        targeted_spell(&passed, UNSUMMON, bears, &[(Blue, 1)]),
    );
    let bounced = resolve_top(&bounce).into_state();
    assert_eq!(physical(&bounced, BEARS).zone(), ObjectZone::Hand(B));
    let removed = resolve_top(&bounced);
    assert_all_targets_illegal(&removed, tap_trigger);
    assert!(removed.state().execution().window().entries().is_empty());
    assert_creature_size(removed.state(), OGRE, 2);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn darksteel_garrison_moves_its_grant_taps_an_indestructible_land_and_detaches_when_the_land_leaves()
 {
    use ManaType::{Blue, Colourless};
    let cast = cast_garrison([]);
    assert!(indestructible(&cast, CITADEL));
    let forest = attach(&cast, FOREST);
    let citadel = attach(&forest, CITADEL);
    assert!(!indestructible(&citadel, FOREST));
    assert!(indestructible(&citadel, CITADEL));
    let stacked = tap_and_target(
        &citadel,
        CITADEL,
        ManaAbilityRef::Printed(ManaAbilityId(0)),
        Colourless,
        OGRE,
    );
    let pumped = resolve_top(&stacked).into_state();
    assert_creature_size(&pumped, OGRE, 3);
    assert_creature_size(&pumped, BEARS, 2);

    let returned = attach(&pumped, FOREST);
    assert!(indestructible(&returned, CITADEL));
    let forest = object(&returned, FOREST);
    let garrison = object(&returned, GARRISON);
    let passed = advance(&returned, A, GameCommand::Pass);
    let bounce = advance(
        &passed,
        B,
        targeted_spell(&passed, BOOMERANG, forest, &[(Blue, 2)]),
    );
    let detached = resolve_top(&bounce).into_state();
    assert_eq!(object(&detached, GARRISON), garrison);
    assert_eq!(
        physical(&detached, GARRISON).zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(physical(&detached, FOREST).zone(), ObjectZone::Hand(B));
    assert_eq!(attachment(&detached), None);
    assert!(indestructible(&detached, CITADEL));
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn darksteel_garrison_fortify_rechecks_control_and_target_identity_at_resolution() {
    use ManaType::{Blue, Colourless};
    let cast = cast_garrison([]);
    let reject_fortify = |state: &GameState, card| {
        assert!(
            state
                .advance(GameInput {
                    basis: state.basis().clone(),
                    actor: A,
                    command: fortify(state, card),
                })
                .is_err()
        );
    };
    reject_fortify(&cast, OPPONENT_LAND);
    let forest = object(&cast, FOREST);
    let announced = advance(&cast, A, fortify(&cast, FOREST));
    reject_fortify(&announced, CITADEL);
    let fortify_entry = announced.execution().window().entries()[0].id();
    let passed = advance(&announced, A, GameCommand::Pass);

    let change_control = advance(
        &passed,
        B,
        targeted_spell(&passed, RETURN_CONTROL, forest, &[]),
    );
    let under_b = resolve_top(&change_control).into_state();
    assert_eq!(physical(&under_b, FOREST).id(), forest);
    assert_eq!(physical(&under_b, FOREST).controller(), Some(B));
    assert_eq!(attachment(&under_b), None);
    let rejected = resolve_top(&under_b);
    assert_all_targets_illegal(&rejected, fortify_entry);
    assert_eq!(attachment(rejected.state()), None);
    assert!(!indestructible(rejected.state(), FOREST));
    assert_eq!(rejected.state().execution().mana(A, Colourless), 7.into());

    let bounce = advance(
        &passed,
        B,
        targeted_spell(&passed, BOOMERANG, forest, &[(Blue, 2)]),
    );
    let bounced = resolve_top(&bounce).into_state();
    assert_ne!(physical(&bounced, FOREST).id(), forest);
    assert_eq!(physical(&bounced, FOREST).zone(), ObjectZone::Hand(B));
    let rejected = resolve_top(&bounced);
    assert_all_targets_illegal(&rejected, fortify_entry);
    assert!(rejected.state().execution().window().entries().is_empty());
    assert_eq!(attachment(rejected.state()), None);
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn darksteel_garrison_loses_its_tap_target_and_detaches_when_the_fortified_land_is_exiled() {
    use super::sacrifice_exile::{exile_response, resolve_exile_response};
    let response = 12;
    let cast = cast_garrison(exile_response(response, 13));
    let fortified = attach(&cast, FOREST);
    assert!(indestructible(&fortified, FOREST));
    let stacked = tap_and_target(
        &fortified,
        FOREST,
        ManaAbilityRef::BasicLand(BasicLandType::Forest),
        ManaType::Green,
        BEARS,
    );
    let trigger = stacked.execution().window().entries()[0].id();
    let garrison = object(&stacked, GARRISON);
    let exiled = resolve_exile_response(&stacked, response, &[(A, FOREST), (B, BEARS)]);

    assert_eq!(physical(&exiled, GARRISON).zone(), ObjectZone::Battlefield);
    assert_eq!(object(&exiled, GARRISON), garrison);
    assert_eq!(attachment(&exiled), None);
    let removed = resolve_top(&exiled);
    assert_all_targets_illegal(&removed, trigger);
    assert!(removed.state().execution().window().entries().is_empty());
    assert_creature_size(removed.state(), OGRE, 2);
}
