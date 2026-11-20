//! CR205.4e/601/707.10 actual cast, current-trait, payment and copy trajectories.
//! Planeswalker card types are still outside the admitted native type family.
#[path = "../test_support/creature_vocabulary.rs"]
mod creature_vocabulary;
#[path = "../test_support/characteristic_trajectories.rs"]
mod tr;
use spellwake_contracts::names::{EnglishCardName, NameRules, ObjectNames};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    characteristics::{
        AbilityChange, AbilityRemoval, ControlRecipient, ResolvedDuration, ResolvedPartSpec,
        SubtypeOperation, TypeChangeSpec, TypeOperation,
    },
    combat::CombatRules,
    effects::{CopyTargetChoice, EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    life::LifeAdjustment,
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaSpend,
        ManaType,
    },
    object_identity::{ObjectDestination, ObjectZone},
    occurrence::{EntryCompletion, EventCause, RulesOccurrence, RulesRecord, ZoneReason},
    program::{
        BodyTarget, CardType, ExecutableBody, Instruction, Program, SpellTargetFilter, Supertype,
    },
    setup::GameSeed,
    stack_window::StackKind,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tr::{P0, P1};
fn named(mut p: Program, label: &str) -> Program {
    p.names = ObjectNames::single(EnglishCardName::new(label.into()).unwrap());
    p
}
fn legendary(mut p: Program) -> Program {
    p.supertypes.insert(Supertype::Legendary);
    p
}
fn creature(is_legend: bool) -> Program {
    let p = named(
        Program::creature(Some(ManaCost::free()), 2.into(), 3.into(), BTreeSet::new()),
        "Keeper",
    );
    if is_legend { legendary(p) } else { p }
}
fn spell(n: u64, sorcery: bool, is_legend: bool) -> SeedCard {
    let body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(3.into()),
        }],
    );
    let p = if sorcery {
        Program::sorcery(ManaCost::one(ManaType::Green), body)
    } else {
        Program::instant(ManaCost::one(ManaType::Green), body)
    };
    tr::card(n, if is_legend { legendary(p) } else { p }, false)
}
fn initial(cards: Vec<SeedCard>) -> GameState {
    let order = cards
        .iter()
        .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
        .map(|c| c.card)
        .collect();
    let seed = GameSeed::with_uniform_life(
        vec![P0, P1],
        P0,
        cards,
        order,
        vec![FloatingMana {
            player: P0,
            mana_type: ManaType::Green,
            amount: 2.into(),
        }],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        tr::vocabulary(),
        Arc::new(NameRules::literal_names()),
    );
    let context = GameContext::new(
        "legendary-casting".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal".into(),
        "legendary-spells".into(),
        "prepared-main".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    GameState::initialize_scheduled(
        GameId(2054),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}
fn payment() -> ManaPayment {
    ManaPayment {
        spend: vec![ManaSpend {
            mana_type: ManaType::Green,
            amount: 1.into(),
        }],
    }
}
fn cast_input(game: &GameState, n: u64) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor: P0,
        command: GameCommand::CastUntargeted {
            spell: tr::object(game, n),
            payment: Some(payment()),
        },
    }
}
fn cast(game: &GameState, n: u64) -> NativeCandidate {
    let input = cast_input(game, n);
    tr::submit(game, P0, input.command)
}
fn reject(game: &GameState, n: u64) {
    let saved = game.clone();
    assert_eq!(
        game.advance(cast_input(game, n)),
        Err(GameError::Effect(EffectError::LegendarySpellPrerequisite))
    );
    assert_eq!(game, &saved);
    assert_eq!(
        game.execution().operational_counters(),
        saved.execution().operational_counters()
    );
}
fn effect(n: u64, parts: Vec<ResolvedPartSpec>, duration: ResolvedDuration) -> SeedCard {
    tr::spell(
        n,
        BodyTarget::SinglePermanent,
        vec![tr::effect(parts, duration)],
    )
}
fn resolve(game: &GameState, n: u64, target: u64) -> NativeCandidate {
    tr::resolve(game, n, tr::object(game, target)).0
}
fn types(supertypes: TypeOperation<Supertype>, types: TypeOperation) -> ResolvedPartSpec {
    ResolvedPartSpec::Type(TypeChangeSpec {
        land_subtypes: SubtypeOperation::Keep,
        supertypes,
        types,
        subtypes: SubtypeOperation::Keep,
    })
}

#[test]
fn both_legendary_spell_kinds_reject_every_nonqualifier_without_any_cost_or_allocation() {
    let mut pretend = creature(false);
    pretend.names = ObjectNames::single(EnglishCardName::new("Legendary Keeper".into()).unwrap());
    let mut in_hand = tr::card(1, creature(true), false);
    in_hand.destination = ObjectDestination::Hand;
    let mut not_bf = tr::card(1, creature(true), false);
    not_bf.destination = ObjectDestination::Graveyard;
    let candidates = vec![
        vec![],
        vec![tr::card(1, creature(false), true)],
        vec![tr::controlled(1, P0, P1, creature(true))],
        vec![tr::card(
            1,
            legendary(Program::artifact(None, vec![])),
            true,
        )],
        vec![in_hand],
        vec![not_bf],
        vec![tr::card(1, pretend, true)],
    ];
    for sorcery in [false, true] {
        for cards in &candidates {
            let mut cards = cards.clone();
            cards.push(spell(10, sorcery, true));
            let game = initial(cards);
            reject(&game, 10);
            assert_eq!(game.execution().mana(P0, ManaType::Green), 2.into());
            assert!(game.execution().window().entries().is_empty());
        }
    }
}

#[test]
fn both_legendary_spell_kinds_accept_a_current_controlled_legend_and_keep_real_payment_receipts() {
    for sorcery in [false, true] {
        let game = initial(vec![
            tr::controlled(1, P1, P0, creature(true)),
            spell(10, sorcery, true),
        ]);
        let candidate = cast(&game, 10);
        let paid = candidate.mana_payment().unwrap();
        assert_eq!(paid.payer, P0);
        assert_eq!(paid.cost, ManaCost::one(ManaType::Green));
        assert_eq!(paid.spend, payment().spend);
        let events: Vec<_> = candidate
            .rules_journal()
            .records
            .iter()
            .filter_map(|record| match record {
                RulesRecord::Event(event) => Some(event),
                _ => None,
            })
            .collect();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0].cause,
            EventCause::CastAnnouncement { entry: paid.source }
        );
        assert_eq!(
            events[1].cause,
            EventCause::ManaDebited { entry: paid.source }
        );
        assert_eq!(
            events[2].cause,
            EventCause::CastCompleted { entry: paid.source }
        );
        assert_ne!(events[0].id, events[1].id);
        assert_ne!(events[1].id, events[2].id);
        assert_ne!(events[0].id, events[2].id);
        assert_eq!(events[0].occurrences.len(), 1);
        let RulesOccurrence::ZoneChange(movement) = &events[0].occurrences[0].value else {
            panic!("one physical announcement required")
        };
        assert_eq!(movement.reason, ZoneReason::CastAnnouncement);
        assert_eq!(movement.before.object.id(), tr::object(&game, 10));
        assert_eq!(movement.before.object.zone(), ObjectZone::Hand(P0));
        assert_eq!(movement.after.object.zone(), ObjectZone::Stack);
        let entry = candidate
            .state()
            .execution()
            .window()
            .entries()
            .last()
            .unwrap();
        assert_eq!(entry.id(), paid.source);
        assert_eq!(
            entry.kind(),
            &StackKind::CardSpell(movement.after.object.id())
        );
        assert!(
            movement
                .before
                .current_supertypes
                .contains(&Supertype::Legendary)
        );
        assert!(
            movement
                .after
                .current_supertypes
                .contains(&Supertype::Legendary)
        );
        assert_eq!(events[1].occurrences.len(), 1);
        assert_eq!(
            events[1].occurrences[0].value,
            RulesOccurrence::ManaDebited {
                payment: paid.clone(),
                mana_type: ManaType::Green,
                amount: 1.into(),
                before: 2.into(),
                after: 1.into(),
            }
        );
        assert_eq!(events[2].occurrences.len(), 1);
        let RulesOccurrence::CastCompleted(cast) = &events[2].occurrences[0].value else {
            panic!("expected the actual completed-cast capture");
        };
        assert_eq!(cast.entry(), paid.source);
        assert_eq!(cast.controller(), P0);
        assert_eq!(cast.spell().object.zone(), ObjectZone::Stack);
        assert!(
            cast.spell()
                .current_supertypes
                .contains(&Supertype::Legendary)
        );
        assert_eq!(
            candidate.state().execution().mana(P0, ManaType::Green),
            1.into()
        );
        assert_eq!(
            candidate
                .state()
                .execution()
                .stack_copiable_supertypes(paid.source),
            Some(&BTreeSet::from([Supertype::Legendary]))
        );
        let done = tr::pass_pair(candidate.state());
        tr::assert_completion(&done, paid.source, EntryCompletion::Resolved);
        assert_eq!(done.state().execution().life_total(P0), Some(&23.into()));
        assert_eq!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(tr::object(done.state(), 10))
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(P0)
        );
    }
}

#[test]
fn ordinary_spells_and_legendary_permanents_or_lands_need_no_qualifying_creature() {
    for sorcery in [false, true] {
        let game = initial(vec![spell(10, sorcery, false)]);
        let paid = cast(&game, 10);
        assert!(paid.mana_payment().is_some());
    }
    let mut relic = tr::card(
        1,
        legendary(named(
            Program::artifact(Some(ManaCost::free()), vec![]),
            "Relic",
        )),
        false,
    );
    relic.destination = ObjectDestination::Hand;
    let land = tr::card(
        2,
        legendary(named(
            Program::land(std::collections::BTreeSet::new(), vec![]),
            "Land",
        )),
        false,
    );
    let game = initial(vec![relic, land]);
    let entered = tr::cast_permanent(&game, 1);
    assert_eq!(
        tr::current(entered.state(), tr::object(entered.state(), 1)).zone,
        ObjectZone::Battlefield
    );
    let played = tr::submit(
        entered.state(),
        P0,
        GameCommand::PlayLand {
            land: tr::object(entered.state(), 2),
        },
    );
    assert!(played.land_play().is_some());
    assert_eq!(
        tr::current(played.state(), tr::object(played.state(), 2)).zone,
        ObjectZone::Battlefield
    );
}

#[test]
fn actual_supertype_gain_and_loss_follow_current_traits_instead_of_printed_flags() {
    let game = initial(vec![
        tr::card(1, creature(false), true),
        spell(10, false, true),
        spell(11, true, true),
        effect(
            20,
            vec![types(
                TypeOperation::Add(BTreeSet::from([Supertype::Legendary])),
                TypeOperation::Keep,
            )],
            ResolvedDuration::UntilEndOfTurn,
        ),
        effect(
            21,
            vec![types(
                TypeOperation::Remove(BTreeSet::from([Supertype::Legendary])),
                TypeOperation::Keep,
            )],
            ResolvedDuration::NoStatedDuration,
        ),
    ]);
    reject(&game, 10);
    let gained = resolve(&game, 20, 1);
    let current = tr::current(gained.state(), tr::object(gained.state(), 1));
    assert!(!current.defined.supertypes.contains(&Supertype::Legendary));
    assert!(current.current_supertypes.contains(&Supertype::Legendary));
    let paid = cast(gained.state(), 10);
    let done = tr::pass_pair(paid.state());
    let lost = resolve(done.state(), 21, 1);
    reject(lost.state(), 11);
}

#[test]
fn legendary_artifact_only_qualifies_while_it_is_currently_a_creature() {
    let game = initial(vec![
        tr::card(1, legendary(Program::artifact(None, vec![])), true),
        spell(10, false, true),
        spell(11, false, true),
        effect(
            20,
            vec![
                types(
                    TypeOperation::Keep,
                    TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                ),
                ResolvedPartSpec::SetPT(tr::pt(2, 3)),
            ],
            ResolvedDuration::NoStatedDuration,
        ),
        effect(
            21,
            vec![types(
                TypeOperation::Keep,
                TypeOperation::Remove(BTreeSet::from([CardType::Creature])),
            )],
            ResolvedDuration::NoStatedDuration,
        ),
    ]);
    reject(&game, 10);
    let animated = resolve(&game, 20, 1);
    let paid = cast(animated.state(), 10);
    let done = tr::pass_pair(paid.state());
    let ended = resolve(done.state(), 21, 1);
    assert!(!tr::current(ended.state(), tr::object(ended.state(), 1)).is_creature());
    reject(ended.state(), 11);
}

#[test]
fn actual_control_changes_use_current_controller_not_physical_owner() {
    let game = initial(vec![
        tr::controlled(1, P1, P1, creature(true)),
        spell(10, false, true),
        spell(11, false, true),
        effect(
            20,
            vec![ResolvedPartSpec::Control(
                ControlRecipient::ResolvingController,
            )],
            ResolvedDuration::NoStatedDuration,
        ),
        effect(
            21,
            vec![ResolvedPartSpec::Control(ControlRecipient::TargetOwner)],
            ResolvedDuration::NoStatedDuration,
        ),
    ]);
    reject(&game, 10);
    let gained = resolve(&game, 20, 1);
    let paid = cast(gained.state(), 10);
    let done = tr::pass_pair(paid.state());
    let lost = resolve(done.state(), 21, 1);
    assert_eq!(
        tr::current(lost.state(), tr::object(lost.state(), 1)).controller,
        Some(P1)
    );
    reject(lost.state(), 11);
}

#[test]
fn losing_all_abilities_does_not_erase_legendary_creature_status() {
    let mut keeper = creature(true);
    keeper.mana.push(ManaAbility {
        id: ManaAbilityId(1),
        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
    });
    let game = initial(vec![
        tr::card(1, keeper, true),
        spell(10, false, true),
        effect(
            20,
            vec![ResolvedPartSpec::Abilities(AbilityChange::remove(
                AbilityRemoval::All,
            ))],
            ResolvedDuration::NoStatedDuration,
        ),
    ]);
    let removed = resolve(&game, 20, 1);
    let c = tr::current(removed.state(), tr::object(removed.state(), 1));
    assert!(c.abilities.mana.is_empty());
    assert!(c.is_creature() && c.current_supertypes.contains(&Supertype::Legendary));
    let paid = cast(removed.state(), 10);
    assert_eq!(paid.state().execution().mana(P0, ManaType::Green), 1.into());
}

#[test]
fn cleanup_expiry_removes_temporary_qualification_for_new_casts() {
    let game = initial(vec![
        tr::card(1, creature(false), true),
        spell(10, false, true),
        effect(
            20,
            vec![types(
                TypeOperation::Add(BTreeSet::from([Supertype::Legendary])),
                TypeOperation::Keep,
            )],
            ResolvedDuration::UntilEndOfTurn,
        ),
    ]);
    let gained = resolve(&game, 20, 1);
    assert!(
        tr::current(gained.state(), tr::object(gained.state(), 1))
            .current_supertypes
            .contains(&Supertype::Legendary)
    );
    let end = tr::reach_end(gained.state(), [P0, P1]);
    let next = tr::pass_pair(&end);
    assert_eq!(next.state().turn().unwrap().position().step, Step::Upkeep);
    let priority = tr::submit(next.state(), P1, GameCommand::Pass);
    reject(priority.state(), 10);
}

#[test]
fn sacrificing_the_qualifier_after_announcement_allows_spell_copy_and_both_resolutions() {
    let mut keeper = creature(true);
    keeper.activated.push(ActivatedAbility {
        id: ActivatedAbilityId(1),
        mana_cost: ManaCost::free(),
        source_cost: SourceCost::Sacrifice,
        body: ExecutableBody::new(
            BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
            vec![Instruction::CopySpell],
        ),
    });
    let mut damage = tr::spell(
        10,
        BodyTarget::SingleCreature,
        vec![Instruction::Damage { amount: 1.into() }],
    );
    damage.program = legendary(Program::instant(
        ManaCost::one(ManaType::Green),
        ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![Instruction::Damage { amount: 1.into() }],
        ),
    ));
    let game = initial(vec![
        tr::card(1, keeper, true),
        tr::controlled(2, P1, P1, creature(false)),
        damage,
        spell(11, false, true),
    ]);
    let announced = tr::submit(
        &game,
        P0,
        GameCommand::CastPaid {
            spell: tr::object(&game, 10),
            target: TargetRef::Object(tr::object(&game, 2)),
            payment: payment(),
        },
    );
    let original = announced.mana_payment().unwrap().source;
    let sacrificed = tr::submit(
        announced.state(),
        P0,
        GameCommand::ActivateAbility {
            source: tr::object(announced.state(), 1),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Stack(original),
            payment: Some(ManaPayment::default()),
        },
    );
    assert_eq!(
        sacrificed
            .state()
            .execution()
            .window()
            .cards()
            .get(tr::object(sacrificed.state(), 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(P0)
    );
    reject(sacrificed.state(), 11);
    let waiting = tr::pass_pair(sacrificed.state());
    assert!(matches!(
        waiting.state().pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    let copied = tr::reply(
        waiting.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Retain),
    );
    let copy = copied
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert!(matches!(copy.kind(), StackKind::SpellCopy { .. }));
    assert_eq!(
        copied
            .state()
            .execution()
            .stack_copiable_supertypes(copy.id()),
        Some(&BTreeSet::from([Supertype::Legendary]))
    );
    assert!(copied.mana_payment().is_none());
    assert_eq!(
        copied.state().execution().mana(P0, ManaType::Green),
        1.into()
    );
    let first = tr::pass_pair(copied.state());
    assert_eq!(
        first
            .state()
            .execution()
            .creature(tr::object(first.state(), 2))
            .unwrap()
            .unwrap()
            .damage,
        1.into()
    );
    let second = tr::pass_pair(first.state());
    tr::assert_completion(&second, original, EntryCompletion::Resolved);
    assert_eq!(
        second
            .state()
            .execution()
            .creature(tr::object(second.state(), 2))
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );
    assert!(second.state().execution().window().entries().is_empty());
    let copies = tr::all_occurrences(&copied)
        .into_iter()
        .filter(|v| matches!(v, RulesOccurrence::CopyCreated { .. }))
        .count();
    assert_eq!(copies, 1);
}
