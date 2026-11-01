//! Independent native acceptance for the fixed tap Land slice. Expected outcomes
//! were frozen before implementation in mana-abilities/reviews/native-scenarios.md.
//! These authored programs do not qualify whole cards, games or nested mana timing.
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    card_identity::{
        CardDestination, CardId, CardIdentityError, CardObjectId, CardObjects, CardZone,
    },
    damage::DamageJournal,
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    game::{
        CandidateBase, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate,
    },
    mana::{
        FixedManaAbility, FloatingMana, ManaAbilityId, ManaCost, ManaPayment, ManaSpend, ManaType,
    },
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackWindowError, WindowPhase},
    targets::TargetRef,
};
use std::collections::BTreeMap;

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const TYPES: [ManaType; 6] = [
    ManaType::White,
    ManaType::Blue,
    ManaType::Black,
    ManaType::Red,
    ManaType::Green,
    ManaType::Colourless,
];

fn context() -> GameContext {
    context_fields(["release", "rules", "catalogue", "programs", "tap-mana"])
}
fn context_fields(fields: [&str; 5]) -> GameContext {
    GameContext::new(
        fields[0].into(),
        fields[1].into(),
        fields[2].into(),
        fields[3].into(),
        fields[4].into(),
    )
    .unwrap()
}
fn output(amounts: &[(ManaType, i64)]) -> BTreeMap<ManaType, RulesInteger> {
    amounts
        .iter()
        .map(|&(mana_type, amount)| (mana_type, amount.into()))
        .collect()
}
fn exact(decimal: &str) -> RulesInteger {
    decimal.to_owned().try_into().unwrap()
}
fn ability(id: u64, amounts: &[(ManaType, i64)]) -> FixedManaAbility {
    FixedManaAbility {
        id: ManaAbilityId(id),
        produced: output(amounts),
    }
}
fn land(
    card: u64,
    owner: SeatId,
    controller: SeatId,
    abilities: Vec<FixedManaAbility>,
) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: CardDestination::Battlefield { controller },
        program: Program::Land { abilities },
    }
}
fn instant(card: u64, owner: SeatId, cost: ManaCost, instruction: Instruction) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: CardDestination::Hand,
        program: Program::Instant {
            cost,
            instructions: vec![instruction],
        },
    }
}
fn creature() -> SeedCard {
    SeedCard {
        card: CardId(90),
        owner: B,
        destination: CardDestination::Battlefield { controller: B },
        program: Program::VanillaCreature {
            power: 2.into(),
            toughness: 100.into(),
        },
    }
}
fn pools(player: SeatId, amounts: &[(ManaType, i64)]) -> Vec<FloatingMana> {
    amounts
        .iter()
        .map(|&(mana_type, amount)| FloatingMana {
            player,
            mana_type,
            amount: amount.into(),
        })
        .collect()
}
fn nonzero_pools() -> Vec<FloatingMana> {
    let mut mana = pools(
        A,
        &[
            (ManaType::White, 1),
            (ManaType::Blue, 2),
            (ManaType::Black, 3),
            (ManaType::Red, 4),
            (ManaType::Green, 5),
            (ManaType::Colourless, 6),
        ],
    );
    mana.extend(pools(
        B,
        &[
            (ManaType::White, 11),
            (ManaType::Blue, 12),
            (ManaType::Black, 13),
            (ManaType::Red, 14),
            (ManaType::Green, 15),
            (ManaType::Colourless, 16),
        ],
    ));
    mana
}
fn setup(cards: Vec<SeedCard>, mana: Vec<FloatingMana>) -> GameState {
    let candidate =
        GameState::initialize(GameId(71), context(), vec![A, B], A, cards, mana).unwrap();
    assert!(candidate.mana_production().is_none());
    assert!(candidate.mana_payment().is_none());
    candidate.into_state()
}
fn one_source() -> GameState {
    setup(
        vec![land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])])],
        nonzero_pools(),
    )
}
fn object(game: &GameState, card: u64) -> CardObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}
fn activate(game: &GameState, card: u64, ability: u64) -> GameCommand {
    GameCommand::ActivateMana {
        source: object(game, card),
        ability: ManaAbilityId(ability),
    }
}
fn request(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}
fn accept(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let saved = game.clone();
    let candidate = game.advance(request(game, actor, command)).unwrap();
    assert_eq!(game, &saved);
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(game.basis().clone())
    );
    assert_eq!(candidate.state().basis().game, game.basis().game);
    assert_eq!(candidate.state().basis().context, game.basis().context);
    assert_eq!(candidate.state().basis().epoch, game.basis().epoch);
    assert_eq!(
        candidate.state().basis().revision,
        game.basis().revision + 1
    );
    candidate
}
fn reject(game: &GameState, input: GameInput, error: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(input), Err(error));
    assert_eq!(game, &saved);
}
fn reject_effect(game: &GameState, actor: SeatId, command: GameCommand, error: EffectError) {
    reject(
        game,
        request(game, actor, command),
        GameError::Effect(error),
    );
}
fn balances(game: &GameState) -> Vec<RulesInteger> {
    [A, B]
        .into_iter()
        .flat_map(|player| TYPES.map(|mana_type| game.execution().mana(player, mana_type)))
        .collect()
}
fn expected_balances(a: [i64; 6], b: [i64; 6]) -> Vec<RulesInteger> {
    a.into_iter().chain(b).map(RulesInteger::from).collect()
}
fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected ready priority")
    };
    holder
}
fn no_production(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let candidate = accept(game, actor, command);
    assert!(candidate.mana_production().is_none());
    candidate
}
fn pass(game: &GameState) -> GameState {
    let before = balances(game);
    let candidate = no_production(game, holder(game), GameCommand::Pass);
    assert!(candidate.mana_payment().is_none());
    assert_eq!(balances(candidate.state()), before);
    candidate.into_state()
}
fn pass_round(game: &GameState) -> GameState {
    pass(&pass(game))
}
fn production(
    candidate: &NativeCandidate,
    before: &GameState,
    card: u64,
    id: u64,
    controller: SeatId,
    produced: &BTreeMap<ManaType, RulesInteger>,
) {
    let record = candidate
        .mana_production()
        .expect("accepted activation has current production evidence");
    let source = object(before, card);
    assert_eq!(record.source, source);
    assert_eq!(record.ability, ManaAbilityId(id));
    assert_eq!(record.controller, controller);
    assert_eq!(&record.produced, produced);
    assert!(candidate.mana_payment().is_none());
    let old = before.execution().window().cards().get(source).unwrap();
    let current = candidate
        .state()
        .execution()
        .window()
        .cards()
        .get(source)
        .unwrap();
    assert_eq!(current.id(), old.id());
    assert_eq!(current.card(), old.card());
    assert_eq!(current.owner(), old.owner());
    assert_eq!(current.controller(), old.controller());
    assert_eq!(current.zone(), CardZone::Battlefield);
    assert_eq!(old.tapped(), Some(false));
    assert_eq!(current.tapped(), Some(true));
    assert_eq!(
        candidate
            .state()
            .execution()
            .window()
            .cards()
            .iter()
            .count(),
        before.execution().window().cards().iter().count()
    );
    for other in before.execution().window().cards().iter() {
        if other.id() != source {
            assert_eq!(
                candidate
                    .state()
                    .execution()
                    .window()
                    .cards()
                    .get(other.id()),
                Some(other)
            );
        }
    }
    assert_eq!(
        candidate.state().execution().window().entries(),
        before.execution().window().entries()
    );
    assert_eq!(
        candidate.state().execution().window().spell_copies(),
        before.execution().window().spell_copies()
    );
    assert_eq!(holder(candidate.state()), controller);
    assert!(candidate.state().pending_decision().is_none());
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
}

#[test]
fn each_mana_type_is_credited_exactly_once_without_stack_or_payment() {
    for mana_type in TYPES {
        let produced = output(&[(mana_type, 1)]);
        let mut mana = pools(A, &[(mana_type, 2)]);
        mana.extend(pools(B, &[(mana_type, 9)]));
        let game = setup(
            vec![land(1, A, A, vec![ability(7, &[(mana_type, 1)])])],
            mana,
        );
        let candidate = accept(&game, A, activate(&game, 1, 7));
        production(&candidate, &game, 1, 7, A, &produced);
        for checked in TYPES {
            assert_eq!(
                candidate.state().execution().mana(A, checked),
                if checked == mana_type {
                    3.into()
                } else {
                    0.into()
                }
            );
            assert_eq!(
                candidate.state().execution().mana(B, checked),
                if checked == mana_type {
                    9.into()
                } else {
                    0.into()
                }
            );
        }
    }
}

#[test]
fn one_fixed_multi_output_ability_adds_all_six_types_and_preserves_other_player() {
    let amounts = [
        (ManaType::White, 2),
        (ManaType::Blue, 3),
        (ManaType::Black, 4),
        (ManaType::Red, 5),
        (ManaType::Green, 6),
        (ManaType::Colourless, 7),
    ];
    let mut mana = pools(
        A,
        &[
            (ManaType::White, 11),
            (ManaType::Blue, 13),
            (ManaType::Black, 17),
            (ManaType::Red, 19),
            (ManaType::Green, 23),
            (ManaType::Colourless, 29),
        ],
    );
    mana.extend(pools(
        B,
        &[
            (ManaType::White, 11),
            (ManaType::Blue, 12),
            (ManaType::Black, 13),
            (ManaType::Red, 14),
            (ManaType::Green, 15),
            (ManaType::Colourless, 16),
        ],
    ));
    let game = setup(vec![land(1, A, A, vec![ability(7, &amounts)])], mana);
    let candidate = accept(&game, A, activate(&game, 1, 7));
    production(&candidate, &game, 1, 7, A, &output(&amounts));
    assert_eq!(
        balances(candidate.state()),
        expected_balances([13, 16, 21, 24, 29, 36], [11, 12, 13, 14, 15, 16])
    );
}

#[test]
fn huge_output_addition_and_record_remain_exact() {
    let initial = exact("100000000000000000000000000000000000003");
    let amount = exact("900000000000000000000000000000000000007");
    let expected = exact("1000000000000000000000000000000000000010");
    let produced = BTreeMap::from([(ManaType::Colourless, amount)]);
    let game = setup(
        vec![land(
            1,
            A,
            A,
            vec![FixedManaAbility {
                id: ManaAbilityId(7),
                produced: produced.clone(),
            }],
        )],
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::Colourless,
                amount: initial,
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Colourless,
                amount: 9.into(),
            },
        ],
    );
    let candidate = accept(&game, A, activate(&game, 1, 7));
    production(&candidate, &game, 1, 7, A, &produced);
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Colourless),
        expected
    );
    assert_eq!(
        candidate.state().execution().mana(B, ManaType::Colourless),
        9.into()
    );
}

#[test]
fn selected_alternative_is_explicit_and_cannot_be_reused_after_source_taps() {
    let game = setup(
        vec![land(
            1,
            A,
            A,
            vec![
                ability(2, &[(ManaType::White, 1)]),
                ability(7, &[(ManaType::Blue, 2)]),
            ],
        )],
        nonzero_pools(),
    );
    let candidate = accept(&game, A, activate(&game, 1, 7));
    production(&candidate, &game, 1, 7, A, &output(&[(ManaType::Blue, 2)]));
    assert_eq!(
        balances(candidate.state()),
        expected_balances([1, 4, 3, 4, 5, 6], [11, 12, 13, 14, 15, 16])
    );
    let tapped = candidate.into_state();
    for id in [2, 7] {
        reject_effect(
            &tapped,
            A,
            activate(&tapped, 1, id),
            EffectError::Window(StackWindowError::SourceTapped),
        );
    }
}

#[test]
fn separate_sources_accumulate_pool_but_each_record_contains_only_current_yield() {
    let game = setup(
        vec![
            land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])]),
            land(2, A, A, vec![ability(7, &[(ManaType::Red, 3)])]),
        ],
        nonzero_pools(),
    );
    let first = accept(&game, A, activate(&game, 1, 7));
    production(&first, &game, 1, 7, A, &output(&[(ManaType::Red, 2)]));
    let game = first.into_state();
    let second = accept(&game, A, activate(&game, 2, 7));
    production(&second, &game, 2, 7, A, &output(&[(ManaType::Red, 3)]));
    assert_eq!(
        balances(second.state()),
        expected_balances([1, 2, 3, 9, 5, 6], [11, 12, 13, 14, 15, 16])
    );
}

#[test]
fn controlling_player_receives_mana_and_retains_priority_even_when_owner_and_active_differ() {
    let game = setup(
        vec![land(1, A, B, vec![ability(7, &[(ManaType::Green, 2)])])],
        nonzero_pools(),
    );
    reject_effect(
        &game,
        A,
        activate(&game, 1, 7),
        EffectError::Window(StackWindowError::WrongController),
    );
    let game = pass(&game);
    assert_eq!(holder(&game), B);
    let candidate = accept(&game, B, activate(&game, 1, 7));
    production(&candidate, &game, 1, 7, B, &output(&[(ManaType::Green, 2)]));
    assert_eq!(
        candidate
            .state()
            .execution()
            .window()
            .cards()
            .get(object(&game, 1))
            .unwrap()
            .owner(),
        A
    );
    assert_eq!(
        balances(candidate.state()),
        expected_balances([1, 2, 3, 4, 5, 6], [11, 12, 13, 14, 17, 16])
    );
}

#[test]
fn activation_erases_earlier_passes_without_returning_priority_to_active_player() {
    let game = setup(
        vec![land(1, B, B, vec![ability(7, &[(ManaType::Blue, 1)])])],
        nonzero_pools(),
    );
    let game = pass(&game);
    let candidate = accept(&game, B, activate(&game, 1, 7));
    production(&candidate, &game, 1, 7, B, &output(&[(ManaType::Blue, 1)]));
    let game = pass(candidate.state());
    assert_eq!(holder(&game), A);
    assert!(game.execution().window().entries().is_empty());
    let game = pass(&game);
    assert_eq!(game.execution().window().phase(), WindowPhase::StepEnded);
}

#[test]
fn owning_an_untapped_source_does_not_restore_priority_after_passing() {
    let game = pass(&one_source());
    assert_eq!(holder(&game), B);
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 1))
            .unwrap()
            .tapped(),
        Some(false)
    );
    reject_effect(
        &game,
        A,
        activate(&game, 1, 7),
        EffectError::Window(StackWindowError::Priority(PriorityError::WrongPlayer)),
    );
}

#[test]
fn activating_with_a_live_stack_neither_inserts_a_new_entry_nor_resolves_existing_spell() {
    let game = setup(
        vec![
            land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])]),
            instant(
                2,
                A,
                ManaCost::free(),
                Instruction::Damage { amount: 1.into() },
            ),
            creature(),
        ],
        nonzero_pools(),
    );
    let cast = no_production(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Card(object(&game, 90)),
        },
    );
    assert!(cast.mana_payment().is_some());
    let game = cast.into_state();
    let candidate = accept(&game, A, activate(&game, 1, 7));
    production(&candidate, &game, 1, 7, A, &output(&[(ManaType::Red, 2)]));
    assert_eq!(candidate.state().execution().window().entries().len(), 1);
    assert_eq!(
        candidate
            .state()
            .execution()
            .creature(object(&game, 90))
            .unwrap()
            .damage,
        0.into()
    );
    let game = pass_round(candidate.state());
    assert_eq!(
        game.execution().creature(object(&game, 90)).unwrap().damage,
        1.into()
    );
    assert!(game.execution().window().entries().is_empty());
}

#[test]
fn produced_mana_is_spent_by_real_castpaid_with_distinct_nonrepeated_records() {
    let game = setup(
        vec![
            land(
                1,
                A,
                A,
                vec![ability(7, &[(ManaType::Red, 2), (ManaType::Blue, 1)])],
            ),
            instant(
                2,
                A,
                ManaCost {
                    generic: 1.into(),
                    required: output(&[(ManaType::Red, 1)]),
                },
                Instruction::Damage { amount: 1.into() },
            ),
            creature(),
        ],
        pools(A, &[(ManaType::Red, 1), (ManaType::Blue, 1)]),
    );
    let candidate = accept(&game, A, activate(&game, 1, 7));
    production(
        &candidate,
        &game,
        1,
        7,
        A,
        &output(&[(ManaType::Red, 2), (ManaType::Blue, 1)]),
    );
    let game = candidate.into_state();
    let payment = ManaPayment {
        spend: vec![
            ManaSpend {
                mana_type: ManaType::Red,
                amount: 1.into(),
            },
            ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into(),
            },
        ],
    };
    let cast = no_production(
        &game,
        A,
        GameCommand::CastPaid {
            spell: object(&game, 2),
            target: TargetRef::Card(object(&game, 90)),
            payment,
        },
    );
    let receipt = cast.mana_payment().unwrap();
    assert_eq!(receipt.payer, A);
    assert_eq!(
        receipt.source,
        cast.state()
            .execution()
            .window()
            .entries()
            .last()
            .unwrap()
            .id()
    );
    assert_eq!(
        receipt.spend,
        vec![
            ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into()
            },
            ManaSpend {
                mana_type: ManaType::Red,
                amount: 1.into()
            }
        ]
    );
    assert_eq!(
        balances(cast.state()),
        expected_balances([0, 1, 0, 2, 0, 0], [0; 6])
    );
    let game = pass_round(cast.state());
    assert_eq!(
        game.execution().creature(object(&game, 90)).unwrap().damage,
        1.into()
    );
    assert_eq!(
        game.execution()
            .window()
            .cards()
            .get(object(&game, 2))
            .unwrap()
            .zone(),
        CardZone::Graveyard(A)
    );
    assert_eq!(
        balances(&game),
        expected_balances([0, 1, 0, 2, 0, 0], [0; 6])
    );
}

#[test]
fn public_executor_and_game_input_use_the_same_activation_and_settling_path() {
    let game = one_source();
    let execution = game.execution();
    let saved = execution.clone();
    let activation = execution
        .activate_mana(execution.revision(), A, object(&game, 1), ManaAbilityId(7))
        .unwrap();
    assert_eq!(execution, &saved);
    assert_eq!(activation.production().source, object(&game, 1));
    assert_eq!(
        activation.production().produced,
        output(&[(ManaType::Red, 2)])
    );
    assert_eq!(
        activation.state().window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
    );
    let candidate = accept(&game, A, activate(&game, 1, 7));
    assert_eq!(candidate.mana_production(), Some(activation.production()));
    let next = activation.into_state();
    assert_eq!(
        candidate.state().execution(),
        &next.settle_checks(next.revision()).unwrap()
    );
}

#[test]
fn empty_duplicate_nonpositive_and_mixed_invalid_authored_domains_reject_setup() {
    let domains = vec![
        vec![],
        vec![
            ability(7, &[(ManaType::Red, 1)]),
            ability(7, &[(ManaType::Blue, 1)]),
        ],
        vec![ability(7, &[])],
        vec![ability(7, &[(ManaType::Red, 0)])],
        vec![ability(7, &[(ManaType::Red, -1)])],
        vec![ability(7, &[(ManaType::Green, 2), (ManaType::Red, 0)])],
        vec![ability(7, &[(ManaType::Green, 2), (ManaType::Red, -1)])],
    ];
    for abilities in domains {
        assert_eq!(
            EffectWindow::new(
                vec![A, B],
                A,
                vec![land(1, A, A, abilities)],
                nonzero_pools()
            ),
            Err(EffectError::InvalidSetup)
        );
    }
    let mut card = land(1, A, A, vec![ability(7, &[(ManaType::Red, 1)])]);
    card.destination = CardDestination::Stack { controller: A };
    assert!(matches!(
        EffectWindow::new(vec![A, B], A, vec![card], nonzero_pools()),
        Err(EffectError::InvalidSetup
            | EffectError::Window(StackWindowError::UntrackedInitialStack))
    ));
}

#[test]
fn unknown_ability_and_nonland_sources_cannot_tap_or_produce() {
    let game = setup(
        vec![
            land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])]),
            creature(),
            instant(
                2,
                A,
                ManaCost::free(),
                Instruction::Damage { amount: 1.into() },
            ),
        ],
        nonzero_pools(),
    );
    reject_effect(
        &game,
        A,
        activate(&game, 1, 99),
        EffectError::UnknownManaAbility,
    );
    for card in [2, 90] {
        reject_effect(
            &game,
            A,
            activate(&game, card, 7),
            EffectError::NotManaSource,
        );
    }
}

#[test]
fn land_outside_battlefield_is_not_treated_as_untapped() {
    for destination in [
        CardDestination::Hand,
        CardDestination::Library,
        CardDestination::Graveyard,
        CardDestination::Exile,
        CardDestination::Command,
    ] {
        let mut card = land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])]);
        card.destination = destination;
        let game = setup(vec![card], nonzero_pools());
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(object(&game, 1))
                .unwrap()
                .tapped(),
            None
        );
        reject_effect(
            &game,
            A,
            activate(&game, 1, 7),
            EffectError::Window(StackWindowError::Cards(CardIdentityError::NotPermanent)),
        );
    }
}

#[test]
fn current_basis_does_not_make_an_unknown_source_handle_valid() {
    let game = one_source();
    let first = CardObjects::new(0)
        .seed_card(0, CardId(1), A, CardDestination::Hand)
        .unwrap();
    let absent = first
        .next
        .seed_card(first.next.revision(), CardId(2), A, CardDestination::Hand)
        .unwrap()
        .after
        .id();
    assert!(game.execution().window().cards().get(absent).is_none());
    reject_effect(
        &game,
        A,
        GameCommand::ActivateMana {
            source: absent,
            ability: ManaAbilityId(7),
        },
        EffectError::UnknownCard,
    );
}

#[test]
fn activation_cannot_cross_game_context_epoch_revision_or_actor_boundary() {
    let game = one_source();
    let base = request(&game, A, activate(&game, 1, 7));
    let mut wrong = base.clone();
    wrong.basis.game = GameId(72);
    reject(&game, wrong, GameError::WrongGame);
    for field in 0..5 {
        let mut fields = ["release", "rules", "catalogue", "programs", "tap-mana"];
        fields[field] = "other";
        let mut wrong = base.clone();
        wrong.basis.context = context_fields(fields);
        reject(&game, wrong, GameError::WrongContext);
    }
    let mut wrong = base.clone();
    wrong.basis.epoch = 1;
    reject(&game, wrong, GameError::WrongEpoch);
    let mut wrong = base.clone();
    wrong.basis.revision = 1;
    reject(&game, wrong, GameError::StaleRevision);
    let mut wrong = base;
    wrong.actor = SeatId(99);
    reject(&game, wrong, GameError::UnknownActor);
}

#[test]
fn executor_rejects_stale_checks_pending_and_closed_step_without_resource_change() {
    let mut state = EffectWindow::new(
        vec![A, B],
        A,
        vec![land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])])],
        nonzero_pools(),
    )
    .unwrap();
    let source = state.window().cards().current_identity(CardId(1)).unwrap();
    let saved = state.clone();
    assert_eq!(
        state.activate_mana(state.revision(), A, source, ManaAbilityId(7)),
        Err(EffectError::Window(StackWindowError::Priority(
            PriorityError::ChecksRequired
        )))
    );
    assert_eq!(state, saved);
    state = state.settle_checks(state.revision()).unwrap();
    let saved = state.clone();
    assert_eq!(
        state.activate_mana(0, A, source, ManaAbilityId(7)),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(state, saved);
    state = state.pass(state.revision(), A).unwrap();
    state = state.settle_checks(state.revision()).unwrap();
    state = state.pass(state.revision(), B).unwrap();
    assert_eq!(state.window().phase(), WindowPhase::StepEnded);
    let saved = state.clone();
    assert_eq!(
        state.activate_mana(state.revision(), A, source, ManaAbilityId(7)),
        Err(EffectError::Window(StackWindowError::NotInPriority))
    );
    assert_eq!(state, saved);
}

#[test]
fn activation_during_target_checking_is_explicitly_unsupported() {
    let game = setup(
        vec![
            land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])]),
            instant(
                2,
                A,
                ManaCost::free(),
                Instruction::Damage { amount: 1.into() },
            ),
            creature(),
        ],
        nonzero_pools(),
    );
    let cast = accept(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Card(object(&game, 90)),
        },
    );
    let source = object(cast.state(), 1);
    let mut state = cast.into_state().execution().clone();
    state = state.pass(state.revision(), A).unwrap();
    state = state.settle_checks(state.revision()).unwrap();
    state = state.pass(state.revision(), B).unwrap();
    assert!(matches!(
        state.window().phase(),
        WindowPhase::CheckingTargets(_)
    ));
    let saved = state.clone();
    assert_eq!(
        state.activate_mana(state.revision(), A, source, ManaAbilityId(7)),
        Err(EffectError::Window(StackWindowError::NotInPriority))
    );
    assert_eq!(state, saved);
}

#[test]
fn pending_copy_decision_blocks_both_players_and_reply_does_not_repeat_production() {
    let game = setup(
        vec![
            land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])]),
            land(4, A, A, vec![ability(7, &[(ManaType::Green, 1)])]),
            instant(
                2,
                A,
                ManaCost::free(),
                Instruction::Damage { amount: 1.into() },
            ),
            instant(3, B, ManaCost::free(), Instruction::CopySpell),
            creature(),
        ],
        nonzero_pools(),
    );
    let activated = accept(&game, A, activate(&game, 1, 7));
    let game = activated.into_state();
    let cast = no_production(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Card(object(&game, 90)),
        },
    );
    let original = cast.mana_payment().unwrap().source;
    let game = pass(cast.state());
    let copied = no_production(
        &game,
        B,
        GameCommand::Cast {
            spell: object(&game, 3),
            target: TargetRef::Stack(original),
        },
    );
    let pending = pass_round(copied.state());
    let decision = pending.pending_decision().unwrap();
    assert_eq!(
        pending
            .execution()
            .window()
            .cards()
            .get(object(&pending, 4))
            .unwrap()
            .tapped(),
        Some(false)
    );
    for actor in [A, B] {
        reject(
            &pending,
            request(&pending, actor, activate(&pending, 4, 7)),
            GameError::DecisionRequired,
        );
    }
    assert!(matches!(
        pending.execution().window().phase(),
        WindowPhase::Resolving(_)
    ));
    let execution = pending.execution();
    let saved = execution.clone();
    assert_eq!(
        execution.activate_mana(
            execution.revision(),
            A,
            object(&pending, 4),
            ManaAbilityId(7)
        ),
        Err(EffectError::Window(StackWindowError::NotInPriority))
    );
    assert_eq!(execution, &saved);
    let before = balances(&pending);
    let reply = no_production(
        &pending,
        B,
        GameCommand::Reply {
            decision: decision.id(),
            choice: CopyTargetChoice::Retain.into(),
        },
    );
    assert!(reply.mana_payment().is_none());
    assert_eq!(balances(reply.state()), before);
    assert_eq!(reply.state().execution().window().spell_copies().len(), 1);
}

#[test]
fn pending_damage_order_rejects_activation_and_reply_does_not_repeat_production() {
    let game = setup(
        vec![
            land(1, A, A, vec![ability(7, &[(ManaType::Red, 2)])]),
            land(4, A, A, vec![ability(7, &[(ManaType::Green, 1)])]),
            land(5, B, B, vec![ability(7, &[(ManaType::Blue, 1)])]),
            SeedCard {
                card: CardId(2),
                owner: A,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::free(),
                    instructions: vec![
                        Instruction::CreatePreventionShield { amount: 1.into() },
                        Instruction::CreatePreventionShield { amount: 1.into() },
                        Instruction::Damage { amount: 3.into() },
                    ],
                },
            },
            creature(),
        ],
        nonzero_pools(),
    );
    let activated = accept(&game, A, activate(&game, 1, 7));
    let game = activated.into_state();
    let cast = no_production(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Card(object(&game, 90)),
        },
    );
    let pending = pass_round(cast.state());
    let decision = pending.pending_decision().unwrap();
    let spellwake_engine::game::RuleFrame::DamageOrder(frame) = decision.frame() else {
        panic!("expected damage-order choice")
    };
    let choice = frame.options()[0];
    let before = balances(&pending);
    for (actor, card) in [(A, 4), (B, 5)] {
        assert_eq!(
            pending
                .execution()
                .window()
                .cards()
                .get(object(&pending, card))
                .unwrap()
                .tapped(),
            Some(false)
        );
        reject(
            &pending,
            request(&pending, actor, activate(&pending, card, 7)),
            GameError::DecisionRequired,
        );
        assert_eq!(balances(&pending), before);
    }
    let replied = no_production(
        &pending,
        B,
        GameCommand::Reply {
            decision: decision.id(),
            choice: spellwake_engine::game::DecisionReply::DamageOrder(choice),
        },
    );
    assert!(replied.mana_payment().is_none());
    assert_eq!(balances(replied.state()), before);
}
