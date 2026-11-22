//! Paid search-to-battlefield and existing typecycling control.
use super::*;
use spellwake_engine::{
    effects::LibraryTopReply,
    land::{EntryTap, LandType},
    occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
    program::{EntryCounterAbility, LibrarySearch, SearchCardMatcher, Supertype},
};

fn pending_search() -> GameState {
    let mut cards = seeds();
    cards.push(SeedCard {
        card: CardId(200),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::sorcery(
            ManaCost {
                generic: 1.into(),
                required: BTreeMap::from([(ManaType::Green, 1.into())]),
            },
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(LibrarySearch::to_battlefield(
                        SearchCardMatcher::BasicLand,
                        true,
                    )),
                }],
            ),
        ),
    });
    for (id, basic) in [(201, true), (202, false)] {
        let mut card = land(id, ObjectDestination::LibraryBottom, false);
        card.program = Program::land(BTreeSet::from([LandType::Forest]), vec![]);
        if basic {
            card.program.supertypes.insert(Supertype::Basic);
            card.program
                .entry_counter_abilities
                .push(EntryCounterAbility::FixedPlusOne(1.into()));
            if let spellwake_engine::program::ProgramKind::Permanent(permanent) =
                &mut card.program.kind
            {
                permanent.entry_tap = Some(EntryTap::Always);
            }
        }
        cards.push(card);
    }
    let game = setup(Some(Step::PrecombatMain), cards);
    let green = game.execution().mana(A, ManaType::Green);
    let red = game.execution().mana(A, ManaType::Red);
    let cast = cast(
        &game,
        A,
        200,
        Some(payment(&[(ManaType::Green, 1), (ManaType::Red, 1)])),
    );
    assert_eq!(
        cast.state().execution().mana(A, ManaType::Green),
        green.minus(&1.into())
    );
    assert_eq!(
        cast.state().execution().mana(A, ManaType::Red),
        red.minus(&1.into())
    );
    resolve(cast.state()).into_state()
}
fn choose(state: &GameState, selected: Option<ObjectId>) -> NativeCandidate {
    let ready = state
        .supply_entropy(state.basis(), vec![0; 8])
        .unwrap()
        .into_state();
    accept(
        &ready,
        A,
        GameCommand::Reply {
            decision: ready.pending_decision().unwrap().id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
        },
    )
}
#[test]
fn paid_basic_land_search_enters_tapped_with_entry_replacements_and_shuffles() {
    let state = pending_search();
    let RuleFrame::LibraryTop(frame) = state.pending_decision().unwrap().frame() else {
        panic!("expected search")
    };
    let selected = object(&state, 201);
    assert_eq!(frame.eligible_results(), &[selected]);
    reject(
        &state,
        A,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(object(&state, 202)),
            }),
        },
        EffectError::InvalidLibraryPartition,
    );
    let result = choose(&state, Some(selected));
    let land = physical(result.state(), 201);
    assert_eq!(land.zone(), ObjectZone::Battlefield);
    assert_eq!(land.controller(), Some(A));
    assert_eq!(land.tapped(), Some(true));
    assert_ne!(land.id(), selected);
    assert_eq!(
        result
            .state()
            .execution()
            .power_toughness_counters(land.id())
            .unwrap()
            .plus_one,
        1.into()
    );
    assert_eq!(result.shuffle_journal().len(), 1);
    assert!(result.state().search_reveal().is_none());
    assert!(result.rules_journal().records.iter().any(|record| matches!(record, RulesRecord::Event(event)
        if event.occurrences.iter().any(|occ| matches!(&occ.value, RulesOccurrence::ZoneChange(change)
            if change.reason == ZoneReason::SearchToBattlefield && change.before.object.id() == selected && change.after.object.id() == land.id() && change.after.object.tapped() == Some(true))))));
}
#[test]
fn basic_land_search_can_fail_to_find_and_still_shuffles() {
    let state = pending_search();
    let result = choose(&state, None);
    assert_eq!(physical(result.state(), 201).zone(), ObjectZone::Library(A));
    assert_eq!(result.shuffle_journal().len(), 1);
    assert!(result.state().search_reveal().is_none());
}

#[test]
fn tap_and_sacrifice_cost_is_atomic_and_records_tapped_lki_before_search() {
    let mut cards = seeds();
    let mut bauble = artifact(200, A, Some(ManaCost::generic(1.into())));
    bauble.destination = ObjectDestination::Battlefield { controller: A };
    bauble.program.mana.push(ManaAbility {
        id: ManaAbilityId(80),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });
    bauble.program.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(81),
        mana_cost: ManaCost::generic(2.into()),
        source_cost: SourceCost::TapSacrifice,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(LibrarySearch::to_battlefield(
                    SearchCardMatcher::BasicLand,
                    true,
                )),
            }],
        ),
    });
    cards.push(bauble);
    let mut forest = land(201, ObjectDestination::LibraryBottom, false);
    forest.program.supertypes.insert(Supertype::Basic);
    cards.push(forest);
    let game = setup(Some(Step::PrecombatMain), cards);
    let activation = |g: &GameState, pay| GameCommand::ActivateUntargetedAbility {
        source: object(g, 200),
        ability: ActivatedAbilityId(81),
        payment: pay,
    };
    let saved = game.clone();
    assert!(
        game.advance(input(
            &game,
            A,
            activation(&game, Some(payment(&[(ManaType::Red, 1)])))
        ))
        .is_err()
    );
    assert_eq!(game, saved);
    let tapped = accept(
        &game,
        A,
        GameCommand::ActivateMana {
            source: object(&game, 200),
            ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(80)),
        },
    );
    let tapped_state = tapped.state();
    let saved = tapped_state.clone();
    assert!(
        tapped_state
            .advance(input(
                tapped_state,
                A,
                activation(tapped_state, Some(payment(&[(ManaType::Red, 2)])))
            ))
            .is_err()
    );
    assert_eq!(*tapped_state, saved);
    let original = object(&game, 200);
    let paid = accept(
        &game,
        A,
        activation(&game, Some(payment(&[(ManaType::Red, 2)]))),
    );
    assert_eq!(physical(paid.state(), 200).zone(), ObjectZone::Graveyard(A));
    let events: Vec<_> = paid
        .rules_journal()
        .records
        .iter()
        .filter_map(|r| match r {
            RulesRecord::Event(e) => Some(e),
            _ => None,
        })
        .flat_map(|e| e.occurrences.iter().map(|o| &o.value))
        .collect();
    let tap = events.iter().position(|o| matches!(o, RulesOccurrence::StatusChange { change, .. } if change.before.id() == original && change.before.tapped() == Some(false) && change.after.tapped() == Some(true))).unwrap();
    let sacrifice = events.iter().position(|o| matches!(o, RulesOccurrence::ZoneChange(change) if change.reason == ZoneReason::SacrificeCost && change.before.object.id() == original && change.before.object.tapped() == Some(true))).unwrap();
    assert!(tap < sacrifice);
    let pending = resolve(paid.state()).into_state();
    let selected = object(&pending, 201);
    let result = choose(&pending, Some(selected));
    assert_eq!(
        physical(result.state(), 201).zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(physical(result.state(), 201).tapped(), Some(true));
    assert_eq!(
        physical(result.state(), 200).zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(result.shuffle_journal().len(), 1);
}

#[test]
fn forest_search_accepts_nonbasic_and_preserves_land_entry_tap_rules() {
    let mut cards = seeds();
    cards.push(SeedCard {
        card: CardId(200),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::sorcery(
            ManaCost::one(ManaType::Green),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::SearchLibrary {
                    filter: Box::new(LibrarySearch::land_subtypes_to_battlefield(
                        [LandType::Forest].into(),
                        false,
                    )),
                }],
            ),
        ),
    });
    for (id, kind, basic, enters_tapped) in [
        (201, LandType::Forest, false, true),
        (202, LandType::Forest, true, false),
        (203, LandType::Island, true, false),
    ] {
        let mut card = land(id, ObjectDestination::LibraryBottom, false);
        card.program = Program::land(BTreeSet::from([kind]), vec![]);
        if basic {
            card.program.supertypes.insert(Supertype::Basic);
        }
        if enters_tapped
            && let spellwake_engine::program::ProgramKind::Permanent(permanent) =
                &mut card.program.kind
        {
            permanent.entry_tap = Some(EntryTap::Always);
        }
        cards.push(card);
    }
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast = cast(&game, A, 200, Some(payment(&[(ManaType::Green, 1)])));
    let pending = resolve(cast.state()).into_state();
    let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
        panic!("expected search")
    };
    assert_eq!(
        frame.eligible_results(),
        &[object(&pending, 201), object(&pending, 202)]
    );
    for (id, tapped) in [(201, true), (202, false)] {
        let selected = object(&pending, id);
        let result = choose(&pending, Some(selected));
        let land = physical(result.state(), id);
        assert_eq!(land.zone(), ObjectZone::Battlefield);
        assert_eq!(land.tapped(), Some(tapped));
        assert_eq!(result.shuffle_journal().len(), 1);
        assert!(result.state().search_reveal().is_none());
    }
}
