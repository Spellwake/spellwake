//! Independent fixed-cost payment acceptance cases, frozen before execution in
//! engine-core/reviews/mana-payment-scenarios.md. Authored native programs qualify
//! neither full casting/cards nor Forge's asymmetric per-symbol failure states.
use spellwake_contracts::RulesInteger;
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    effects::{CopyTargetChoice, EffectError, EffectWindow, Instruction, Program, SeedCard},
    game::{
        CandidateBase, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate,
    },
    mana::{FloatingMana, ManaCost, ManaPayment, ManaPaymentError, ManaSpend, ManaType},
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackKind, StackWindowError, WindowPhase},
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
    context_fields(["release", "rules", "catalogue", "programs", "mana-profile"])
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

fn cost(generic: i64, required: &[(ManaType, i64)]) -> ManaCost {
    ManaCost {
        generic: generic.into(),
        required: required
            .iter()
            .map(|&(mana_type, amount)| (mana_type, amount.into()))
            .collect(),
    }
}

fn payment(spend: &[(ManaType, i64)]) -> ManaPayment {
    ManaPayment {
        spend: spend
            .iter()
            .map(|&(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
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

fn instant(id: u64, owner: SeatId, cost: ManaCost, instruction: Instruction) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: CardDestination::Hand,
        program: Program::Instant {
            cost,
            instructions: vec![instruction],
        },
    }
}

fn creature(id: u64, owner: SeatId) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: CardDestination::Battlefield { controller: owner },
        program: Program::VanillaCreature {
            power: 2.into(),
            toughness: 100.into(),
        },
    }
}

fn seeds(cost: ManaCost) -> Vec<SeedCard> {
    vec![
        instant(1, A, cost, Instruction::Damage { amount: 1.into() }),
        creature(90, B),
    ]
}

fn initialize(cost: ManaCost, mana: Vec<FloatingMana>) -> NativeCandidate {
    GameState::initialize(GameId(41), context(), vec![A, B], A, seeds(cost), mana).unwrap()
}

fn setup(cost: ManaCost, mana: Vec<FloatingMana>) -> GameState {
    let candidate = initialize(cost, mana);
    assert!(candidate.mana_payment().is_none());
    candidate.into_state()
}

fn object(game: &GameState, card: u64) -> CardObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn zone(game: &GameState, card: u64) -> CardZone {
    game.execution()
        .window()
        .cards()
        .get(object(game, card))
        .unwrap()
        .zone()
}

fn request(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}

fn command(game: &GameState, spend: ManaPayment) -> GameCommand {
    GameCommand::CastPaid {
        spell: object(game, 1),
        target: TargetRef::Card(object(game, 90)),
        payment: spend,
    }
}

fn convenience(game: &GameState) -> GameCommand {
    GameCommand::Cast {
        spell: object(game, 1),
        target: TargetRef::Card(object(game, 90)),
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

fn rejected_payment(game: &GameState, spend: ManaPayment, error: EffectError) {
    reject(
        game,
        request(game, A, command(game, spend)),
        GameError::Effect(error),
    );
}

fn invalid_payment(error: ManaPaymentError) -> EffectError {
    EffectError::InvalidManaPayment(error)
}

fn balances(game: &GameState) -> Vec<RulesInteger> {
    [A, B]
        .into_iter()
        .flat_map(|player| TYPES.map(|mana_type| game.execution().mana(player, mana_type)))
        .collect()
}

fn receipt(candidate: &NativeCandidate, payer: SeatId, cost: &ManaCost, spend: ManaPayment) {
    let record = candidate
        .mana_payment()
        .expect("accepted cast has one receipt");
    let entry = candidate
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap();
    assert_eq!(record.source, entry.id());
    assert_eq!(record.payer, payer);
    assert_eq!(&record.cost, cost);
    assert_eq!(record.spend, spend.spend);
    let StackKind::CardSpell(object) = entry.kind() else {
        panic!("payment source must be the actual card spell entry")
    };
    assert_eq!(
        candidate
            .state()
            .execution()
            .window()
            .cards()
            .get(*object)
            .unwrap()
            .zone(),
        CardZone::Stack
    );
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected ready priority")
    };
    holder
}

fn noncast(game: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    let before = balances(game);
    let candidate = accept(game, actor, command);
    assert!(candidate.mana_payment().is_none());
    assert_eq!(balances(candidate.state()), before);
    candidate.into_state()
}

fn pass_round(game: GameState) -> GameState {
    let next = noncast(&game, holder(&game), GameCommand::Pass);
    noncast(&next, holder(&next), GameCommand::Pass)
}

#[test]
fn each_of_the_six_specific_types_has_one_exact_debit_and_receipt() {
    for mana_type in TYPES {
        let cost = ManaCost::one(mana_type);
        let mut mana = pools(A, &[(mana_type, 2)]);
        mana.extend(pools(B, &[(mana_type, 9)]));
        let game = setup(cost.clone(), mana);
        let old_source = object(&game, 1);
        let explicit = accept(&game, A, command(&game, payment(&[(mana_type, 1)])));
        let inferred = accept(&game, A, convenience(&game));
        assert_eq!(explicit, inferred);
        receipt(&explicit, A, &cost, payment(&[(mana_type, 1)]));
        assert_ne!(old_source, object(explicit.state(), 1));
        assert!(
            explicit
                .state()
                .execution()
                .window()
                .cards()
                .get(old_source)
                .is_none()
        );
        for observed in TYPES {
            assert_eq!(
                explicit.state().execution().mana(A, observed),
                if observed == mana_type {
                    1.into()
                } else {
                    0.into()
                }
            );
            assert_eq!(
                explicit.state().execution().mana(B, observed),
                if observed == mana_type {
                    9.into()
                } else {
                    0.into()
                }
            );
        }
    }
}

#[test]
fn mixed_mandatory_payment_normalizes_all_six_types_without_following_input_order() {
    let required = TYPES.map(|mana_type| (mana_type, 1));
    let cost = cost(0, &required);
    let available = TYPES.map(|mana_type| (mana_type, 2));
    let game = setup(cost.clone(), pools(A, &available));
    let mut reversed = required;
    reversed.reverse();
    let candidate = accept(&game, A, command(&game, payment(&reversed)));
    let inferred = accept(&game, A, convenience(&game));
    assert_eq!(candidate, inferred);
    receipt(&candidate, A, &cost, payment(&required));
    for mana_type in TYPES {
        assert_eq!(candidate.state().execution().mana(A, mana_type), 1.into());
    }
}

#[test]
fn caller_can_preserve_either_colored_or_colorless_mana_for_the_same_generic_cost() {
    let cost = ManaCost::generic(1.into());
    let game = setup(
        cost.clone(),
        pools(A, &[(ManaType::Red, 1), (ManaType::Colourless, 1)]),
    );
    for (chosen, preserved) in [
        (ManaType::Red, ManaType::Colourless),
        (ManaType::Colourless, ManaType::Red),
    ] {
        let candidate = accept(&game, A, command(&game, payment(&[(chosen, 1)])));
        receipt(&candidate, A, &cost, payment(&[(chosen, 1)]));
        assert_eq!(candidate.state().execution().mana(A, chosen), 0.into());
        assert_eq!(candidate.state().execution().mana(A, preserved), 1.into());
    }
}

#[test]
fn mixed_generic_and_required_payment_preserves_unselected_resource() {
    let cost = cost(2, &[(ManaType::Red, 1), (ManaType::White, 1)]);
    let game = setup(
        cost.clone(),
        pools(
            A,
            &[
                (ManaType::White, 1),
                (ManaType::Blue, 1),
                (ManaType::Red, 1),
                (ManaType::Green, 1),
                (ManaType::Colourless, 1),
            ],
        ),
    );
    let candidate = accept(
        &game,
        A,
        command(
            &game,
            payment(&[
                (ManaType::Red, 1),
                (ManaType::White, 1),
                (ManaType::Colourless, 1),
                (ManaType::Blue, 1),
            ]),
        ),
    );
    receipt(
        &candidate,
        A,
        &cost,
        payment(&[
            (ManaType::White, 1),
            (ManaType::Blue, 1),
            (ManaType::Red, 1),
            (ManaType::Colourless, 1),
        ]),
    );
    for mana_type in TYPES {
        assert_eq!(
            candidate.state().execution().mana(A, mana_type),
            if mana_type == ManaType::Green {
                1.into()
            } else {
                0.into()
            }
        );
    }
}

#[test]
fn surplus_of_a_specific_required_type_can_pay_generic_including_true_colorless() {
    for mana_type in [ManaType::Red, ManaType::Colourless] {
        let cost = cost(1, &[(mana_type, 1)]);
        let game = setup(cost.clone(), pools(A, &[(mana_type, 3)]));
        let candidate = accept(&game, A, command(&game, payment(&[(mana_type, 2)])));
        receipt(&candidate, A, &cost, payment(&[(mana_type, 2)]));
        assert_eq!(candidate.state().execution().mana(A, mana_type), 1.into());
    }
}

#[test]
fn colored_mana_cannot_replace_true_colorless_even_when_both_types_are_available() {
    let game = setup(
        ManaCost::one(ManaType::Colourless),
        pools(A, &[(ManaType::Red, 2), (ManaType::Colourless, 1)]),
    );
    rejected_payment(
        &game,
        payment(&[(ManaType::Red, 1)]),
        invalid_payment(ManaPaymentError::MissingRequired),
    );
}

#[test]
fn omitted_generic_payment_distinguishes_choice_from_mandatory_and_residual_shortage() {
    let cases = [
        (
            ManaCost::generic(1.into()),
            vec![(ManaType::Red, 1)],
            EffectError::ManaPaymentRequired,
        ),
        (
            cost(2, &[(ManaType::Red, 1)]),
            vec![(ManaType::Blue, 100)],
            EffectError::InsufficientMana,
        ),
        (
            cost(2, &[(ManaType::Red, 1)]),
            vec![(ManaType::Red, 1), (ManaType::Blue, 1)],
            EffectError::InsufficientMana,
        ),
        (
            cost(2, &[(ManaType::Red, 1)]),
            vec![(ManaType::Red, 3)],
            EffectError::ManaPaymentRequired,
        ),
    ];
    for (cost, amounts, error) in cases {
        let game = setup(cost, pools(A, &amounts));
        reject(
            &game,
            request(&game, A, convenience(&game)),
            GameError::Effect(error),
        );
        assert!(game.pending_decision().is_none());
    }
}

#[test]
fn every_explicit_zero_cost_form_accepts_an_empty_payment_with_a_receipt() {
    let zero_forms = [
        ManaCost::free(),
        ManaCost::generic(0.into()),
        ManaCost::specific(ManaType::Red, 0.into()),
        cost(0, &[(ManaType::White, 0), (ManaType::Colourless, 0)]),
    ];
    for cost in zero_forms {
        let game = setup(cost.clone(), pools(A, &[(ManaType::Black, 4)]));
        let explicit = accept(&game, A, command(&game, payment(&[])));
        let inferred = accept(&game, A, convenience(&game));
        assert_eq!(explicit, inferred);
        receipt(&explicit, A, &cost, payment(&[]));
        assert_eq!(balances(explicit.state()), balances(&game));
        assert_eq!(zone(explicit.state(), 1), CardZone::Stack);
    }
}

#[test]
fn nonpositive_or_duplicate_spend_entries_are_not_normalized_into_valid_payments() {
    for amount in [0, -1] {
        let game = setup(ManaCost::generic(1.into()), pools(A, &[(ManaType::Red, 4)]));
        rejected_payment(
            &game,
            payment(&[(ManaType::Red, amount)]),
            invalid_payment(ManaPaymentError::NonPositiveSpend),
        );
    }
    let game = setup(ManaCost::generic(2.into()), pools(A, &[(ManaType::Red, 4)]));
    rejected_payment(
        &game,
        payment(&[(ManaType::Red, 1), (ManaType::Red, 1)]),
        invalid_payment(ManaPaymentError::DuplicateType),
    );
}

#[test]
fn exact_total_rejects_underpayment_overpayment_empty_positive_and_free_extra() {
    for (amount, spend) in [(3, 2), (1, 2), (1, 0), (0, 1)] {
        let game = setup(
            ManaCost::generic(amount.into()),
            pools(A, &[(ManaType::Red, 4)]),
        );
        let proposed = if spend == 0 {
            payment(&[])
        } else {
            payment(&[(ManaType::Red, spend)])
        };
        rejected_payment(
            &game,
            proposed,
            invalid_payment(ManaPaymentError::WrongTotal),
        );
    }
}

#[test]
fn correct_total_cannot_hide_a_missing_required_type() {
    let game = setup(
        cost(0, &[(ManaType::Red, 1), (ManaType::White, 1)]),
        pools(A, &[(ManaType::White, 2), (ManaType::Red, 1)]),
    );
    rejected_payment(
        &game,
        payment(&[(ManaType::White, 2)]),
        invalid_payment(ManaPaymentError::MissingRequired),
    );
}

#[test]
fn explicit_selection_must_be_available_to_this_payer() {
    let game = setup(ManaCost::generic(1.into()), pools(A, &[(ManaType::Red, 1)]));
    rejected_payment(
        &game,
        payment(&[(ManaType::Colourless, 1)]),
        EffectError::InsufficientMana,
    );
    let game = setup(
        ManaCost::one(ManaType::Red),
        pools(B, &[(ManaType::Red, 2)]),
    );
    rejected_payment(
        &game,
        payment(&[(ManaType::Red, 1)]),
        EffectError::InsufficientMana,
    );
    reject(
        &game,
        request(&game, A, convenience(&game)),
        GameError::Effect(EffectError::InsufficientMana),
    );
}

#[test]
fn negative_cost_components_and_invalid_pool_seeds_reject_whole_setup() {
    for invalid in [
        cost(-1, &[]),
        cost(0, &[(ManaType::Red, -1)]),
        cost(2, &[(ManaType::Red, -1)]),
    ] {
        assert_eq!(
            EffectWindow::new(
                vec![A, B],
                A,
                seeds(invalid),
                pools(A, &[(ManaType::Red, 4)])
            ),
            Err(EffectError::InvalidSetup)
        );
    }
    for invalid in [
        pools(A, &[(ManaType::Red, -1)]),
        pools(A, &[(ManaType::Red, 1), (ManaType::Red, 2)]),
        pools(SeatId(99), &[(ManaType::Red, 1)]),
    ] {
        assert_eq!(
            EffectWindow::new(vec![A, B], A, seeds(ManaCost::free()), invalid),
            Err(EffectError::InvalidSetup)
        );
    }
}

fn exact(decimal: &str) -> RulesInteger {
    decimal.to_owned().try_into().unwrap()
}

#[test]
fn generic_and_specific_arithmetic_and_receipts_preserve_huge_exact_quantities() {
    let huge = exact("900719925474099312345678901234567890");
    let larger = exact("900719925474099312345678901234567891");
    let cost = ManaCost {
        generic: huge.clone(),
        required: BTreeMap::from([(ManaType::Colourless, 3.into())]),
    };
    let game = setup(
        cost.clone(),
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::Blue,
                amount: larger.clone(),
            },
            FloatingMana {
                player: A,
                mana_type: ManaType::Colourless,
                amount: 5.into(),
            },
        ],
    );
    let proposed = ManaPayment {
        spend: vec![
            ManaSpend {
                mana_type: ManaType::Colourless,
                amount: 3.into(),
            },
            ManaSpend {
                mana_type: ManaType::Blue,
                amount: huge.clone(),
            },
        ],
    };
    let candidate = accept(&game, A, command(&game, proposed));
    receipt(
        &candidate,
        A,
        &cost,
        ManaPayment {
            spend: vec![
                ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: huge.clone(),
                },
                ManaSpend {
                    mana_type: ManaType::Colourless,
                    amount: 3.into(),
                },
            ],
        },
    );
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Blue),
        1.into()
    );
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Colourless),
        2.into()
    );
    let cost = ManaCost::specific(ManaType::Black, huge.clone());
    let game = setup(
        cost.clone(),
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::Black,
            amount: larger,
        }],
    );
    let candidate = accept(&game, A, convenience(&game));
    receipt(
        &candidate,
        A,
        &cost,
        ManaPayment {
            spend: vec![ManaSpend {
                mana_type: ManaType::Black,
                amount: huge,
            }],
        },
    );
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Black),
        1.into()
    );
}

#[test]
fn a_single_unit_of_huge_underpayment_or_overpayment_is_detected_exactly() {
    let game = setup(
        ManaCost::generic(exact("900719925474099312345678901234567890")),
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::Blue,
            amount: exact("900719925474099312345678901234567892"),
        }],
    );
    for amount in [
        "900719925474099312345678901234567889",
        "900719925474099312345678901234567891",
    ] {
        rejected_payment(
            &game,
            ManaPayment {
                spend: vec![ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: exact(amount),
                }],
            },
            invalid_payment(ManaPaymentError::WrongTotal),
        );
    }
}

#[test]
fn a_paid_command_cannot_cross_any_outer_identity_or_revision_boundary() {
    let game = setup(ManaCost::generic(1.into()), pools(A, &[(ManaType::Red, 2)]));
    let make_request = || request(&game, A, command(&game, payment(&[(ManaType::Red, 1)])));
    let mut wrong = make_request();
    wrong.basis.game = GameId(42);
    reject(&game, wrong, GameError::WrongGame);
    let original = ["release", "rules", "catalogue", "programs", "mana-profile"];
    for changed in 0..original.len() {
        let mut fields = original;
        fields[changed] = "different";
        let mut wrong = make_request();
        wrong.basis.context = context_fields(fields);
        reject(&game, wrong, GameError::WrongContext);
    }
    let mut wrong = make_request();
    wrong.basis.epoch = 1;
    reject(&game, wrong, GameError::WrongEpoch);
    let mut wrong = make_request();
    wrong.basis.revision = 1;
    reject(&game, wrong, GameError::StaleRevision);
    let mut wrong = make_request();
    wrong.actor = SeatId(99);
    reject(&game, wrong, GameError::UnknownActor);
    let accepted = accept(&game, A, command(&game, payment(&[(ManaType::Red, 1)])));
    reject(accepted.state(), make_request(), GameError::StaleRevision);
}

#[test]
fn valid_payment_does_not_bypass_own_hand_program_target_or_holder_checks() {
    let mut cards = seeds(ManaCost::one(ManaType::Red));
    cards.push(instant(
        2,
        B,
        ManaCost::one(ManaType::Red),
        Instruction::Damage { amount: 1.into() },
    ));
    let mut hand_creature = creature(3, A);
    hand_creature.destination = CardDestination::Hand;
    cards.push(hand_creature);
    let mut mana = pools(A, &[(ManaType::Red, 4)]);
    mana.extend(pools(B, &[(ManaType::Red, 4)]));
    let game = GameState::initialize(GameId(41), context(), vec![A, B], A, cards, mana)
        .unwrap()
        .into_state();
    let target = TargetRef::Card(object(&game, 90));
    for (spell, error) in [(2, EffectError::NotOwnHand), (3, EffectError::NotInstant)] {
        reject(
            &game,
            request(
                &game,
                A,
                GameCommand::CastPaid {
                    spell: object(&game, spell),
                    target,
                    payment: payment(&[(ManaType::Red, 1)]),
                },
            ),
            GameError::Effect(error),
        );
    }
    let other = setup(ManaCost::free(), vec![]);
    let foreign_cast = accept(&other, A, convenience(&other));
    let missing_stack_entry = foreign_cast.mana_payment().unwrap().source;
    for target in [
        TargetRef::Player(A),
        TargetRef::Stack(missing_stack_entry),
        TargetRef::Card(object(&game, 1)),
    ] {
        reject(
            &game,
            request(
                &game,
                A,
                GameCommand::CastPaid {
                    spell: object(&game, 1),
                    target,
                    payment: payment(&[(ManaType::Red, 1)]),
                },
            ),
            GameError::Effect(EffectError::IllegalTarget),
        );
    }
    reject(
        &game,
        request(
            &game,
            B,
            GameCommand::CastPaid {
                spell: object(&game, 2),
                target,
                payment: payment(&[(ManaType::Red, 1)]),
            },
        ),
        GameError::Effect(EffectError::Window(StackWindowError::Priority(
            PriorityError::WrongPlayer,
        ))),
    );
}

#[test]
fn unknown_source_handle_cannot_use_a_valid_payment_or_current_game_basis() {
    let game = setup(
        ManaCost::one(ManaType::Red),
        pools(A, &[(ManaType::Red, 3)]),
    );
    let other = GameState::initialize(
        GameId(42),
        context(),
        vec![A, B],
        A,
        vec![creature(10, B), creature(11, B), creature(12, B)],
        vec![],
    )
    .unwrap()
    .into_state();
    let foreign = object(&other, 12);
    assert!(game.execution().window().cards().get(foreign).is_none());
    reject(
        &game,
        request(
            &game,
            A,
            GameCommand::CastPaid {
                spell: foreign,
                target: TargetRef::Card(object(&game, 90)),
                payment: payment(&[(ManaType::Red, 1)]),
            },
        ),
        GameError::Effect(EffectError::UnknownCard),
    );
}

#[test]
fn old_source_identity_is_not_retargeted_to_the_accepted_stack_incarnation() {
    let game = setup(
        ManaCost::one(ManaType::Red),
        pools(A, &[(ManaType::Red, 3)]),
    );
    let old = object(&game, 1);
    let candidate = accept(&game, A, command(&game, payment(&[(ManaType::Red, 1)])));
    let next = candidate.into_state();
    reject(
        &next,
        request(
            &next,
            A,
            GameCommand::CastPaid {
                spell: old,
                target: TargetRef::Card(object(&next, 90)),
                payment: payment(&[(ManaType::Red, 1)]),
            },
        ),
        GameError::Effect(EffectError::UnknownCard),
    );
    reject(
        &next,
        request(
            &next,
            A,
            GameCommand::CastPaid {
                spell: object(&next, 1),
                target: TargetRef::Card(object(&next, 90)),
                payment: payment(&[(ManaType::Red, 1)]),
            },
        ),
        GameError::Effect(EffectError::NotOwnHand),
    );
}

#[test]
fn departed_target_and_its_new_hand_incarnation_both_reject_without_payment() {
    let mut cards = seeds(ManaCost::one(ManaType::Red));
    cards.push(instant(
        2,
        A,
        ManaCost::free(),
        Instruction::ReturnToOwnersHand,
    ));
    let game = GameState::initialize(
        GameId(41),
        context(),
        vec![A, B],
        A,
        cards,
        pools(A, &[(ManaType::Red, 3)]),
    )
    .unwrap()
    .into_state();
    let old = object(&game, 90);
    let cast = accept(
        &game,
        A,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Card(old),
        },
    );
    let game = pass_round(cast.into_state());
    assert_eq!(zone(&game, 90), CardZone::Hand(B));
    assert_ne!(old, object(&game, 90));
    for target in [old, object(&game, 90)] {
        reject(
            &game,
            request(
                &game,
                A,
                GameCommand::CastPaid {
                    spell: object(&game, 1),
                    target: TargetRef::Card(target),
                    payment: payment(&[(ManaType::Red, 1)]),
                },
            ),
            GameError::Effect(EffectError::IllegalTarget),
        );
    }
}

#[test]
fn low_level_paid_transition_and_legacy_wrapper_share_the_same_successor() {
    let cost = cost(0, &[(ManaType::Red, 1), (ManaType::Colourless, 1)]);
    let game = setup(
        cost.clone(),
        pools(A, &[(ManaType::Red, 2), (ManaType::Colourless, 3)]),
    );
    let state = game.execution();
    let saved = state.clone();
    let spell = object(&game, 1);
    let target = TargetRef::Card(object(&game, 90));
    let transition = state
        .cast_with_payment(
            state.revision(),
            A,
            spell,
            target,
            Some(payment(&[(ManaType::Colourless, 1), (ManaType::Red, 1)])),
        )
        .unwrap();
    let inferred = state
        .cast_with_payment(state.revision(), A, spell, target, None)
        .unwrap();
    assert_eq!(transition, inferred);
    assert_eq!(
        transition.state(),
        &state.cast(state.revision(), A, spell, target).unwrap()
    );
    assert_eq!(transition.payment().cost, cost);
    assert_eq!(
        transition.payment().spend,
        payment(&[(ManaType::Red, 1), (ManaType::Colourless, 1)]).spend
    );
    let accepted = transition.state().clone();
    assert_eq!(transition.into_state(), accepted);
    assert_eq!(state, &saved);
}

#[test]
fn low_level_stale_revision_checks_required_and_closed_step_preserve_resources() {
    let mut state = EffectWindow::new(
        vec![A, B],
        A,
        seeds(ManaCost::one(ManaType::Red)),
        pools(A, &[(ManaType::Red, 3)]),
    )
    .unwrap();
    let spell = state.window().cards().current_identity(CardId(1)).unwrap();
    let target = TargetRef::Card(state.window().cards().current_identity(CardId(90)).unwrap());
    let saved = state.clone();
    assert_eq!(
        state.cast_with_payment(
            state.revision(),
            A,
            spell,
            target,
            Some(payment(&[(ManaType::Red, 1)]))
        ),
        Err(EffectError::Window(StackWindowError::Priority(
            PriorityError::ChecksRequired
        )))
    );
    assert_eq!(state, saved);
    state = state.settle_checks(state.revision()).unwrap();
    let saved = state.clone();
    assert_eq!(
        state.cast_with_payment(0, A, spell, target, Some(payment(&[(ManaType::Red, 1)]))),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(state, saved);
    state = state.pass(state.revision(), A).unwrap();
    state = state.settle_checks(state.revision()).unwrap();
    state = state.pass(state.revision(), B).unwrap();
    assert_eq!(state.window().phase(), WindowPhase::StepEnded);
    let saved = state.clone();
    assert_eq!(
        state.cast_with_payment(
            state.revision(),
            A,
            spell,
            target,
            Some(payment(&[(ManaType::Red, 1)]))
        ),
        Err(EffectError::Window(StackWindowError::NotInPriority))
    );
    assert_eq!(state, saved);
}

#[test]
fn copy_reply_and_countered_source_neither_repeat_receipts_charge_copies_nor_refund_costs() {
    let cards = vec![
        instant(
            1,
            A,
            ManaCost::generic(1.into()),
            Instruction::Damage { amount: 1.into() },
        ),
        instant(2, B, ManaCost::one(ManaType::Blue), Instruction::CopySpell),
        instant(
            3,
            A,
            ManaCost::one(ManaType::Red),
            Instruction::CounterSpell,
        ),
        creature(90, B),
    ];
    let mut mana = pools(A, &[(ManaType::Green, 1), (ManaType::Red, 1)]);
    mana.extend(pools(B, &[(ManaType::Blue, 1)]));
    let game = GameState::initialize(GameId(41), context(), vec![A, B], A, cards, mana)
        .unwrap()
        .into_state();
    let first = accept(&game, A, command(&game, payment(&[(ManaType::Green, 1)])));
    receipt(
        &first,
        A,
        &ManaCost::generic(1.into()),
        payment(&[(ManaType::Green, 1)]),
    );
    let original = first.mana_payment().unwrap().source;
    let game = first.into_state();
    let game = noncast(&game, A, GameCommand::Pass);
    let copy = accept(
        &game,
        B,
        GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Stack(original),
        },
    );
    receipt(
        &copy,
        B,
        &ManaCost::one(ManaType::Blue),
        payment(&[(ManaType::Blue, 1)]),
    );
    let pending = pass_round(copy.into_state());
    let decision = pending.pending_decision().unwrap();
    for actor in [A, B] {
        for command in [
            GameCommand::Cast {
                spell: object(&pending, 3),
                target: TargetRef::Stack(original),
            },
            GameCommand::CastPaid {
                spell: object(&pending, 3),
                target: TargetRef::Stack(original),
                payment: payment(&[(ManaType::Red, 1)]),
            },
        ] {
            reject(
                &pending,
                request(&pending, actor, command),
                GameError::DecisionRequired,
            );
        }
    }
    let game = noncast(
        &pending,
        B,
        GameCommand::Reply {
            decision: decision.id(),
            choice: CopyTargetChoice::Retain.into(),
        },
    );
    assert_eq!(game.execution().window().spell_copies().len(), 1);
    assert_eq!(zone(&game, 2), CardZone::Graveyard(B));
    let counter = accept(
        &game,
        A,
        GameCommand::CastPaid {
            spell: object(&game, 3),
            target: TargetRef::Stack(original),
            payment: payment(&[(ManaType::Red, 1)]),
        },
    );
    receipt(
        &counter,
        A,
        &ManaCost::one(ManaType::Red),
        payment(&[(ManaType::Red, 1)]),
    );
    let game = pass_round(counter.into_state());
    assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
    assert_eq!(zone(&game, 3), CardZone::Graveyard(A));
    assert_eq!(game.execution().window().entries().len(), 1);
    let game = pass_round(game);
    assert!(game.execution().window().spell_copies().is_empty());
    assert!(game.execution().window().entries().is_empty());
    assert_eq!(
        game.execution().creature(object(&game, 90)).unwrap().damage,
        1.into()
    );
    assert_eq!(balances(&game), vec![RulesInteger::from(0); 12]);
    let game = pass_round(game);
    assert_eq!(game.execution().window().phase(), WindowPhase::StepEnded);
    assert_eq!(balances(&game), vec![RulesInteger::from(0); 12]);
}
