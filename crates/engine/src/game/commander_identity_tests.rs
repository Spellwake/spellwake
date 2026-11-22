//! Commander identity choice and empty-production behavior at the game boundary.
use super::*;
use crate::{
    characteristics::Color,
    mana::{ManaAbility, ManaAbilityRef, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination},
    program::Program,
    test_support::creature_types,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

fn game() -> GameState {
    let (a, b) = (SeatId(0), SeatId(1));
    let vocabulary = creature_types::synthetic_vocabulary();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        "identity-test".into(),
        vocabulary.rules_revision().into(),
        "synthetic".into(),
        "synthetic".into(),
        "synthetic".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let mut land = Program::land(BTreeSet::new(), vec![]);
    land.mana.push(ManaAbility {
        id: ManaAbilityId(0),
        output: ManaOutput::SpecialColor(crate::mana::SpecialColorYield::CommanderColorIdentity),
    });
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        vec![
            SeedCard {
                card: CardId(1),
                owner: b,
                destination: ObjectDestination::Battlefield { controller: a },
                program: land,
            },
            SeedCard {
                card: CardId(2),
                owner: a,
                destination: ObjectDestination::Command,
                program: Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            },
        ],
        vec![CardId(1)],
        vec![],
        40.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    GameState::initialize_scheduled(
        GameId(303),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn activate(state: &GameState) -> Result<NativeCandidate, GameError> {
    let source = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    state.advance(GameInput {
        basis: state.basis().clone(),
        actor: SeatId(0),
        command: GameCommand::ActivateMana {
            source,
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        },
    })
}
#[test]
fn empty_commander_identity_taps_without_mana_and_missing_input_is_rejected() {
    let a = SeatId(0);
    let b = SeatId(1);
    let mut state = game();
    state
        .execution
        .designate_commanders(BTreeSet::from([CardId(2)]));
    let original = state.clone();
    assert_eq!(
        activate(&state),
        Err(GameError::Effect(
            EffectError::MissingCommanderColorIdentity
        ))
    );
    assert_eq!(state, original);
    state
        .execution
        .set_commander_color_identities(BTreeMap::from([
            (a, BTreeSet::new()),
            (b, BTreeSet::from([Color::Red])),
        ]));
    let result = activate(&state).unwrap();
    assert!(result.mana_production().unwrap().produced.is_empty());
    assert!(result.state().pending_decision().is_none());
    let source = result
        .state()
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    assert_eq!(
        result
            .state()
            .execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert!(
        ManaType::ALL
            .into_iter()
            .all(|kind| result.state().execution().mana(a, kind) == 0.into())
    );
}
