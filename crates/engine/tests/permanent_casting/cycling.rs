use super::*;
use spellwake_engine::{
    characteristics::KeywordAbility,
    life::LifeAdjustment,
    occurrence::{EventCause, RulesOccurrence, RulesRecord, ZoneReason},
    program::{TriggerGrouping, TriggerSpec, TriggeredAbility, TriggeredAbilityId},
};

const ID: ActivatedAbilityId = ActivatedAbilityId(32);

fn cycle(game: &GameState, card: u64) -> GameCommand {
    GameCommand::Cycle {
        source: object(game, card),
        ability: ID,
        payment: None,
    }
}
fn cards() -> Vec<SeedCard> {
    let mut cards = seeds();
    for n in [1, 3, 10] {
        cards
            .iter_mut()
            .find(|s| s.card == CardId(n))
            .unwrap()
            .program
            .activated
            .push(ActivatedAbility::cycling(ID, ManaCost::one(ManaType::Blue)));
    }
    cards
}
fn triggered(id: u64, condition: TriggerSpec) -> TriggeredAbility {
    TriggeredAbility {
        id: TriggeredAbilityId(id),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            }],
        ),
    }
}
#[test]
fn cycling_pays_and_discards_before_the_draw_and_accepts_responses() {
    for card in [1, 10] {
        let game = setup(Some(Step::Upkeep), cards());
        let source = object(&game, card);
        let before = game.execution().mana(A, ManaType::Blue);
        let paid = accept(&game, A, cycle(&game, card));
        let activation = paid.ability_activation().unwrap();
        assert_eq!(activation.source.id(), source);
        assert_eq!(activation.source.zone(), ObjectZone::Hand(A));
        assert_eq!(
            physical(paid.state(), card).zone(),
            ObjectZone::Graveyard(A)
        );
        assert_ne!(object(paid.state(), card), source);
        assert_eq!(
            paid.state().execution().mana(A, ManaType::Blue),
            before.minus(&1.into())
        );
        assert!(paid.draw_journal().events.is_empty());
        assert_eq!(paid.state().execution().window().entries().len(), 1);
        let entry = paid.state().execution().window().entries()[0].id();
        assert!(paid.rules_journal().records.iter().any(|r| matches!(r,RulesRecord::Event(e)
            if matches!(e.cause, EventCause::ActivatedAbilityCost {entry: id, ability: ID} if id == entry)
            && e.occurrences.iter().any(|occ| matches!(&occ.value,RulesOccurrence::ZoneChange(z)
                if z.reason == (ZoneReason::CycleDiscard {player:A}) && z.before.object.id() == source && z.after.object.id() == activation.resulting_source)))));
        let other = pass(paid.state()).into_state();
        reject(
            &other,
            B,
            GameCommand::Cast {
                spell: object(&other, 11),
                target: TargetRef::Stack(entry),
            },
            EffectError::IllegalTarget,
        );
        let response = accept(&other, B, cycle(&other, 3));
        assert_eq!(response.state().execution().window().entries().len(), 2);
        let theirs = resolve(response.state());
        assert_eq!(physical(theirs.state(), 99).zone(), ObjectZone::Hand(B));
        assert_eq!(physical(theirs.state(), 98).zone(), ObjectZone::Library(A));
        let ours = resolve(theirs.state());
        assert_eq!(physical(ours.state(), 98).zone(), ObjectZone::Hand(A));
        assert_eq!(ours.draw_journal().events.len(), 1);
    }
}
#[test]
fn split_second_ownership_mana_and_current_zone_restrict_cycling() {
    let mut defs = cards();
    defs.iter_mut()
        .find(|s| s.card == CardId(13))
        .unwrap()
        .program
        .printed_keywords
        .insert(KeywordAbility::SplitSecond);
    let game = setup(Some(Step::PrecombatMain), defs);
    reject(&game, A, cycle(&game, 3), EffectError::NotOwnHand);
    let cast = targeted(&game, A, 13, TargetRef::Object(object(&game, 90)));
    reject(
        cast.state(),
        A,
        cycle(cast.state(), 1),
        EffectError::SplitSecond,
    );
    let completed = resolve(cast.state());
    let paid = accept(completed.state(), A, cycle(completed.state(), 1));
    let rejected = cycle(paid.state(), 1);
    reject(paid.state(), A, rejected, EffectError::NotOwnHand);
    let empty = initialize(Some(Step::PrecombatMain), cards(), vec![]).into_state();
    reject(&empty, A, cycle(&empty, 1), EffectError::InsufficientMana);
}
#[test]
fn self_cycled_trigger_is_controlled_by_the_owner_and_resolves_before_the_draw() {
    let mut defs = cards();
    defs.iter_mut()
        .find(|s| s.card == CardId(10))
        .unwrap()
        .program
        .triggered
        .push(triggered(
            1,
            spellwake_engine::program::TriggerSpec::player_action(
                spellwake_engine::program::PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::Cycled { self_only: true },
            )
            .in_zone(spellwake_engine::program::TriggerZone::Any),
        ));
    let game = setup(Some(Step::Upkeep), defs);
    let activation = accept(&game, A, cycle(&game, 10));
    let entries = activation.state().execution().window().entries();
    assert_eq!(entries.len(), 2);
    assert!(
        matches!(entries[1].kind(),StackKind::TriggeredAbility {source} if source.id()==object(activation.state(),10) && source.zone()==ObjectZone::Graveyard(A))
    );
    let trigger = resolve(activation.state());
    assert_eq!(
        trigger.state().execution().life_total(A),
        Some(&game.execution().life_total(A).unwrap().plus(&1.into()))
    );
    assert_eq!(physical(trigger.state(), 98).zone(), ObjectZone::Library(A));
    assert_eq!(
        physical(resolve(trigger.state()).state(), 98).zone(),
        ObjectZone::Hand(A)
    );
}

#[test]
fn the_cycling_cost_fires_both_discard_and_cycle_observers_for_the_actual_player() {
    use spellwake_engine::program::PlayerRelation;
    for condition in [
        spellwake_engine::program::TriggerSpec::player_action(
            PlayerRelation::Opponent,
            spellwake_engine::program::PlayerTriggerAction::Cycled { self_only: false },
        ),
        spellwake_engine::program::TriggerSpec::player_action(
            PlayerRelation::Opponent,
            spellwake_engine::program::PlayerTriggerAction::Discarded,
        ),
    ] {
        let mut defs = cards();
        defs.iter_mut()
            .find(|s| s.card == CardId(90))
            .unwrap()
            .program
            .triggered
            .push(triggered(2, condition));
        let game = setup(Some(Step::Upkeep), defs);
        let activated = accept(&game, A, cycle(&game, 1));
        let entries = activated.state().execution().window().entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].controller(), B);
        assert!(
            matches!(entries[1].kind(),StackKind::TriggeredAbility { source } if source.id()==object(&game,90))
        );
        let triggered = resolve(activated.state());
        assert_eq!(
            triggered.state().execution().life_total(B),
            Some(&game.execution().life_total(B).unwrap().plus(&1.into()))
        );
    }
}

use spellwake_engine::{
    effects::{LibraryChoiceKind, LibraryTopReply},
    land::LandType,
    program::{LibrarySearch, SearchCardMatcher, Supertype},
};

fn search_state(filter: LibrarySearch) -> GameState {
    let mut cards = seeds();
    cards[0]
        .program
        .activated
        .push(ActivatedAbility::typecycling(ID, ManaCost::free(), filter));
    for (id, subtype, basic) in [
        (100, LandType::Island, false),
        (101, LandType::Island, true),
        (102, LandType::Forest, true),
    ] {
        let mut card = land(id, ObjectDestination::LibraryBottom, false);
        card.program = Program::land(BTreeSet::from([subtype]), vec![]);
        if basic {
            card.program.supertypes.insert(Supertype::Basic);
        }
        cards.push(card);
    }
    let game = setup(Some(Step::Upkeep), cards);
    let activated = accept(&game, A, cycle(&game, 1));
    resolve(activated.state()).into_state()
}

fn search_reply(state: &GameState, selected: Option<ObjectId>) -> GameCommand {
    GameCommand::Reply {
        decision: state.pending_decision().unwrap().id(),
        choice: DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
    }
}

#[test]
fn typecycling_uses_the_searchers_library_actual_subtypes_and_host_shuffle() {
    let state = search_state(LibrarySearch::land_subtype_to_hand(LandType::Island));
    let RuleFrame::LibraryTop(frame) = state.pending_decision().unwrap().frame() else {
        panic!("expected search");
    };
    assert_eq!(frame.kind(), LibraryChoiceKind::Search);
    assert_eq!(frame.chooser(), A);
    assert_eq!(
        frame.domain_top_first(),
        state.execution().window().cards().library(A)
    );
    let selected = object(&state, 100);
    assert_eq!(frame.eligible_results(), &[selected, object(&state, 101)]);
    let valid = search_reply(&state, Some(selected));
    reject_input(
        &state,
        input(&state, B, valid.clone()),
        GameError::WrongChooser,
    );
    for card in [98, 99, 102] {
        reject(
            &state,
            A,
            search_reply(&state, Some(object(&state, card))),
            EffectError::InvalidLibraryPartition,
        );
    }
    let unchanged = state.clone();
    reject(
        &state,
        A,
        valid.clone(),
        EffectError::Window(spellwake_engine::stack_window::StackWindowError::Objects(
            spellwake_engine::object_identity::ObjectIdentityError::Entropy(
                spellwake_engine::shuffle::EntropyError::Exhausted,
            ),
        )),
    );
    assert_eq!(state, unchanged);
    let supplied = state
        .supply_entropy(state.basis(), vec![u64::MAX, 7, 19, 11])
        .unwrap()
        .into_state();
    reject_input(
        &supplied,
        input(&supplied, A, valid),
        GameError::WrongDecision,
    );
    let result = accept(&supplied, A, search_reply(&supplied, Some(selected)));
    assert_eq!(result.state().pending_decision(), None);
    assert_eq!(physical(result.state(), 100).zone(), ObjectZone::Hand(A));
    assert_ne!(selected, object(result.state(), 100));
    assert_eq!(
        result.state().execution().window().cards().library(B),
        state.execution().window().cards().library(B)
    );
    let [shuffle] = result.shuffle_journal() else {
        panic!("expected exactly one shuffle")
    };
    assert_eq!(shuffle.player, A);
    assert_eq!(
        shuffle.before,
        vec![object(&state, 98), object(&state, 101), object(&state, 102)]
    );
    assert_eq!(
        shuffle.after,
        result.state().execution().window().cards().library(A)
    );
    assert_eq!(shuffle.entropy_end - shuffle.entropy_start, 3); // The first word is rejected for a three-card bound.
    assert_eq!(result.state().search_reveal().unwrap().0.id(), selected);
    assert!(pass(result.state()).state().search_reveal().is_none());
    assert!(result.draw_journal().events.is_empty());
    assert!(result.rules_journal().records.iter().any(|r| matches!(r,RulesRecord::Event(event)
        if event.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(change)
            if change.reason==ZoneReason::SearchToHand && change.before.object.id()==selected && change.after.object.id()==object(result.state(),100))))));
}

#[test]
fn searching_may_find_nothing_even_when_matching_basic_lands_exist() {
    let state = search_state(LibrarySearch::to_hand(SearchCardMatcher::BasicLand));
    let RuleFrame::LibraryTop(frame) = state.pending_decision().unwrap().frame() else {
        panic!("expected search");
    };
    assert_eq!(
        frame.eligible_results(),
        &[object(&state, 101), object(&state, 102)]
    );
    let state = state
        .supply_entropy(state.basis(), vec![0; 4])
        .unwrap()
        .into_state();
    let result = accept(&state, A, search_reply(&state, None));
    let [shuffle] = result.shuffle_journal() else {
        panic!("expected a shuffle of the full library")
    };
    assert_eq!(shuffle.before.len(), 4);
    assert!(result.state().search_reveal().is_none());
    assert_eq!(shuffle.entropy_end - shuffle.entropy_start, 3);
    assert!(!result.rules_journal().records.iter().any(|r| matches!(r,RulesRecord::Event(event)
        if event.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(change) if change.reason==ZoneReason::SearchToHand)))));
}

fn transmute(game: &GameState, card: u64) -> GameCommand {
    GameCommand::Transmute {
        source: object(game, card),
        ability: ID,
        payment: None,
    }
}

#[test]
fn transmute_pays_discards_as_a_sorcery_and_searches_for_equal_mana_value() {
    let mut definitions = seeds();
    for card in &mut definitions {
        if [CardId(1), CardId(3)].contains(&card.card) {
            let mana_value = card.program.mana_value();
            card.program.activated.push(ActivatedAbility::transmute(
                ID,
                ManaCost::one(ManaType::Blue),
                mana_value,
            ));
        }
    }
    let mut matching = artifact(103, A, Some(ManaCost::specific(ManaType::Red, 2.into())));
    matching.destination = ObjectDestination::LibraryBottom;
    definitions.push(matching);
    for step in [Step::PrecombatMain, Step::PostcombatMain] {
        let game = setup(Some(step), definitions.clone());
        reject(
            &game,
            A,
            cycle(&game, 1),
            EffectError::UnknownActivatedAbility,
        );
        let other_turn = pass(&game).into_state();
        reject(
            &other_turn,
            B,
            transmute(&other_turn, 3),
            EffectError::Timing(TimingError::NotActivePlayer),
        );
        let casting = targeted(&game, A, 13, TargetRef::Object(object(&game, 90)));
        reject(
            casting.state(),
            A,
            transmute(casting.state(), 1),
            EffectError::Timing(TimingError::StackNotEmpty),
        );
        let card = object(&game, 1);
        let paid = accept(&game, A, transmute(&game, 1));
        assert_eq!(paid.ability_activation().unwrap().source.id(), card);
        assert_eq!(physical(paid.state(), 1).zone(), ObjectZone::Graveyard(A));
        assert_eq!(
            paid.state().execution().mana(A, ManaType::Blue),
            game.execution().mana(A, ManaType::Blue).minus(&1.into())
        );
        assert!(paid.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|
            matches!(&o.value,RulesOccurrence::ZoneChange(change) if change.before.object.id()==card && change.reason==ZoneReason::TransmuteDiscard{player:A})))));
        let pending = resolve(paid.state()).into_state();
        let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
            panic!("expected mana-value search")
        };
        assert_eq!(frame.eligible_results(), &[object(&pending, 103)]);
        let found = accept(
            &pending,
            A,
            search_reply(&pending, Some(object(&pending, 103))),
        );
        assert_eq!(physical(found.state(), 103).zone(), ObjectZone::Hand(A));
        assert!(found.state().search_reveal().is_some());
        assert_eq!(found.shuffle_journal()[0].entropy_end, 0);
    }
    let upkeep = setup(Some(Step::Upkeep), definitions);
    reject(
        &upkeep,
        A,
        transmute(&upkeep, 1),
        EffectError::Timing(TimingError::MainPhaseRequired),
    );
}

#[test]
fn empty_property_searches_finish_without_consuming_entropy() {
    let mut definitions = seeds();
    definitions.retain(|c| c.card != CardId(98));
    definitions[0]
        .program
        .activated
        .push(ActivatedAbility::typecycling(
            ID,
            ManaCost::free(),
            LibrarySearch::land_subtype_to_hand(LandType::Mountain),
        ));
    let source = setup(Some(Step::Upkeep), definitions);
    let pending = resolve(accept(&source, A, cycle(&source, 1)).state()).into_state();
    let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
        panic!("expected empty search")
    };
    assert!(frame.domain_top_first().is_empty() && frame.eligible_results().is_empty());
    let result = accept(&pending, A, search_reply(&pending, None));
    assert_eq!(result.shuffle_journal()[0].entropy_end, 0);
    assert!(result.state().search_reveal().is_none());
}

#[test]
fn creature_typecycling_can_find_a_changeling_of_the_named_type_in_the_library() {
    let vocabulary = test_vocabulary();
    let kind = vocabulary.resolve_label("Elf").unwrap();
    let mut definitions = seeds();
    definitions[0]
        .program
        .activated
        .push(ActivatedAbility::typecycling(
            ID,
            ManaCost::free(),
            LibrarySearch::to_hand(SearchCardMatcher::CreatureSubtype(kind)),
        ));
    let mut printed = creature(100, A, ObjectDestination::LibraryBottom, None, 3);
    printed.program.creature_subtypes.insert(kind);
    let mut changeling = creature(101, A, ObjectDestination::LibraryBottom, None, 3);
    changeling
        .program
        .printed_keywords
        .insert(KeywordAbility::Changeling);
    definitions.extend([printed, changeling]);
    let source = setup(Some(Step::Upkeep), definitions);
    let pending = resolve(accept(&source, A, cycle(&source, 1)).state()).into_state();
    let RuleFrame::LibraryTop(frame) = pending.pending_decision().unwrap().frame() else {
        panic!("expected a subtype search");
    };
    assert_eq!(
        frame.eligible_results(),
        &[object(&pending, 100), object(&pending, 101)]
    );
}

#[test]
fn reinforce_discards_from_hand_and_counters_the_target_only_on_resolution() {
    let mut definitions = seeds();
    definitions[0]
        .program
        .activated
        .push(ActivatedAbility::reinforce(
            ID,
            ManaCost::one(ManaType::Green),
            2.into(),
        ));
    let source = setup(Some(Step::Upkeep), definitions);
    let creature = object(&source, 90);
    let original = object(&source, 1);
    let reinforce = |target| GameCommand::ActivateAbility {
        source: original,
        ability: ID,
        target,
        payment: None,
    };
    reject(
        &source,
        A,
        reinforce(TargetRef::Player(B)),
        EffectError::IllegalTarget,
    );
    let paid = accept(&source, A, reinforce(TargetRef::Object(creature)));
    assert_eq!(physical(paid.state(), 1).zone(), ObjectZone::Graveyard(A));
    assert_eq!(
        paid.state().execution().mana(A, ManaType::Green),
        source.execution().mana(A, ManaType::Green).minus(&1.into())
    );
    assert!(
        paid.state()
            .execution()
            .power_toughness_counters(creature)
            .is_none_or(|c| c.plus_one == 0.into())
    );
    let resolved = resolve(paid.state());
    assert_eq!(
        resolved
            .state()
            .execution()
            .power_toughness_counters(creature)
            .unwrap()
            .plus_one,
        2.into()
    );
    let first = pass(paid.state()).into_state();
    // The ability survives loss of its source, but a returned target is a new object.
    let answer = GameCommand::Cast {
        spell: object(&first, 11),
        target: TargetRef::Stack(paid.ability_activation().unwrap().entry),
    };
    reject(&first, B, answer, EffectError::IllegalTarget);
    let response = targeted(paid.state(), A, 13, TargetRef::Object(creature));
    let gone = resolve(response.state());
    let done = resolve(gone.state());
    assert_eq!(physical(done.state(), 90).zone(), ObjectZone::Hand(B));
    assert!(
        done.state()
            .execution()
            .power_toughness_counters(object(done.state(), 90))
            .is_none()
    );
    assert_eq!(physical(done.state(), 1).zone(), ObjectZone::Graveyard(A));
}

fn unearth_cards(toughness: i64) -> Vec<SeedCard> {
    let mut cards = seeds();
    let mut card = creature(
        100,
        A,
        ObjectDestination::Graveyard,
        Some(ManaCost::one(ManaType::Blue)),
        toughness,
    );
    card.program
        .activated
        .push(ActivatedAbility::unearth(ID, ManaCost::one(ManaType::Red)));
    cards.push(card);
    cards
}
fn unearth(game: &GameState) -> GameCommand {
    GameCommand::ActivateUntargetedAbility {
        source: object(game, 100),
        ability: ID,
        payment: None,
    }
}
fn to_end(game: &GameState) -> GameState {
    assert_eq!(game.turn().unwrap().position().step, Step::PostcombatMain);
    let next = resolve(game).into_state();
    assert_eq!(next.turn().unwrap().position().step, Step::End);
    next
}
#[test]
fn unearth_grants_haste_replaces_departures_and_triggers_once_at_the_next_end_step() {
    let game = setup(Some(Step::PostcombatMain), unearth_cards(2));
    let paid = accept(&game, A, unearth(&game));
    assert_eq!(physical(paid.state(), 100).zone(), ObjectZone::Graveyard(A));
    assert_eq!(
        paid.state().execution().mana(A, ManaType::Red),
        game.execution().mana(A, ManaType::Red).minus(&1.into())
    );
    let returned = resolve(paid.state()).into_state();
    let permanent = object(&returned, 100);
    assert_eq!(physical(&returned, 100).zone(), ObjectZone::Battlefield);
    assert!(physical(&returned, 100).is_unearthed());
    assert!(
        returned
            .execution()
            .current_characteristics(permanent)
            .unwrap()
            .unwrap()
            .has_keyword(KeywordAbility::Haste)
    );
    let pending = to_end(&returned);
    assert_eq!(pending.execution().window().entries().len(), 1);
    assert!(
        matches!(pending.execution().window().entries()[0].kind(),StackKind::TriggeredAbility{source} if source.id()==object(&game,100))
    );
    let finished = resolve(&pending);
    assert_eq!(physical(finished.state(), 100).zone(), ObjectZone::Exile);
    assert!(!physical(finished.state(), 100).is_unearthed());
    let bounce = targeted(&returned, A, 13, TargetRef::Object(permanent));
    let gone = resolve(bounce.state()).into_state();
    assert_eq!(physical(&gone, 100).zone(), ObjectZone::Exile);
    let exiled = object(&gone, 100);
    let delayed = to_end(&gone);
    assert_eq!(delayed.execution().window().entries().len(), 1);
    assert_eq!(object(resolve(&delayed).state(), 100), exiled);
    let upkeep = setup(Some(Step::Upkeep), unearth_cards(2));
    reject(
        &upkeep,
        A,
        unearth(&upkeep),
        EffectError::Timing(TimingError::MainPhaseRequired),
    );
}

#[test]
fn unearth_replacement_competes_in_the_sba_reducer_and_applies_only_once() {
    use spellwake_engine::program::{
        PublicZoneDestination as Dest, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    };
    for reverse in [false, true] {
        let mut definitions = unearth_cards(0);
        if reverse {
            let other = definitions
                .iter_mut()
                .find(|c| c.card == CardId(90))
                .unwrap();
            other
                .program
                .zone_replacements
                .push(ZoneReplacementAbility {
                    id: ZoneReplacementAbilityId(9),
                    actions: ZoneActionPredicate::OneOf(BTreeSet::from([
                        ZoneActionKind::StateBased,
                    ])),
                    objects: ZoneObjectKind::Card,
                    subject: ZoneReplacementSubject::AnyObject,
                    destination: Dest::Exile,
                    replace_with: Dest::Graveyard,
                    choice: ZoneReplacementChoice::Mandatory,
                });
        }
        let game = setup(Some(Step::PostcombatMain), definitions);
        let paid = accept(&game, A, unearth(&game));
        let completed = resolve(paid.state());
        let expected = if reverse {
            ObjectZone::Graveyard(A)
        } else {
            ObjectZone::Exile
        };
        assert_eq!(physical(completed.state(), 100).zone(), expected);
        assert_ne!(object(completed.state(), 100), object(&game, 100));
    }
}
