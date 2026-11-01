//! Independent acceptance scenarios for the restricted, game-scoped transition driver.
//! These observe native private state and do not qualify transport, persistence or full cards.
use spellwake_engine::{
    card_identity::{CardDestination, CardId, CardObjectId, CardZone},
    effects::{CopyTargetChoice, EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionId, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState, NativeCandidate, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaType},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, WindowPhase},
    targets::TargetRef,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn context() -> GameContext {
    make_context([
        "release-1",
        "rules-1",
        "catalogue-1",
        "programs-1",
        "profile-1",
    ])
}

fn make_context(fields: [&str; 5]) -> GameContext {
    GameContext::new(
        fields[0].into(),
        fields[1].into(),
        fields[2].into(),
        fields[3].into(),
        fields[4].into(),
    )
    .unwrap()
}

fn instant(
    id: u64,
    owner: SeatId,
    mana_type: ManaType,
    amount: i64,
    instruction: Instruction,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: CardDestination::Hand,
        program: Program::Instant {
            cost: ManaCost::specific(mana_type, amount.into()),
            instructions: vec![instruction],
        },
    }
}

fn cards() -> Vec<SeedCard> {
    let mut cards = vec![
        instant(
            1,
            A,
            ManaType::Red,
            1,
            Instruction::Damage { amount: 3.into() },
        ),
        instant(2, B, ManaType::Blue, 2, Instruction::CopySpell),
        instant(3, A, ManaType::Blue, 2, Instruction::CounterSpell),
        instant(4, A, ManaType::Blue, 1, Instruction::ReturnToOwnersHand),
    ];
    for (id, owner) in [(5, B), (6, A)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination: CardDestination::Battlefield { controller: owner },
            program: Program::VanillaCreature {
                power: 2.into(),
                toughness: 2.into(),
            },
        });
    }
    cards
}

fn mana() -> Vec<FloatingMana> {
    vec![
        FloatingMana {
            player: A,
            mana_type: ManaType::Red,
            amount: 1.into(),
        },
        FloatingMana {
            player: A,
            mana_type: ManaType::Blue,
            amount: 3.into(),
        },
        FloatingMana {
            player: B,
            mana_type: ManaType::Blue,
            amount: 2.into(),
        },
    ]
}

fn initialize(game: GameId, context: GameContext) -> NativeCandidate {
    GameState::initialize(game, context, vec![A, B], A, cards(), mana()).unwrap()
}

fn setup() -> GameState {
    initialize(GameId(10), context()).into_state()
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

fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}

fn advance(game: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    let saved = game.clone();
    let candidate = game.advance(input(game, actor, command)).unwrap();
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
    assert_eq!(game, &saved);
    candidate.into_state()
}

fn reject(game: &GameState, request: GameInput, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(expected));
    assert_eq!(game, &saved);
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected a settled priority boundary")
    };
    holder
}

fn holding(mut game: GameState, actor: SeatId) -> GameState {
    for _ in 0..2 {
        if holder(&game) == actor {
            return game;
        }
        game = advance(&game, holder(&game), GameCommand::Pass);
    }
    panic!("actor did not receive priority")
}

fn cast(game: GameState, actor: SeatId, card: u64, target: TargetRef) -> GameState {
    let game = holding(game, actor);
    advance(
        &game,
        actor,
        GameCommand::Cast {
            spell: object(&game, card),
            target,
        },
    )
}

fn pass_round(game: GameState) -> GameState {
    let game = advance(&game, holder(&game), GameCommand::Pass);
    advance(&game, holder(&game), GameCommand::Pass)
}

fn stacked() -> GameState {
    let game = setup();
    let target = object(&game, 5);
    let game = cast(game, A, 1, TargetRef::Card(target));
    let original = game.execution().window().entries()[0].id();
    cast(game, B, 2, TargetRef::Stack(original))
}

fn pending() -> GameState {
    let game = pass_round(stacked());
    assert!(game.pending_decision().is_some());
    game
}

fn missing_target() -> (GameState, CardObjectId) {
    let game = stacked();
    let old = object(&game, 5);
    let game = pass_round(cast(game, A, 4, TargetRef::Card(old)));
    assert_eq!(zone(&game, 5), CardZone::Hand(B));
    assert_ne!(old, object(&game, 5));
    (pass_round(game), old)
}

fn reply(game: &GameState, choice: CopyTargetChoice) -> GameState {
    let decision = game.pending_decision().unwrap();
    advance(
        game,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice: choice.into(),
        },
    )
}

fn copied() -> GameState {
    let game = pending();
    reply(
        &game,
        CopyTargetChoice::Change(TargetRef::Card(object(&game, 6))),
    )
}

fn copy_id(game: &GameState) -> StackEntryId {
    *game
        .execution()
        .window()
        .spell_copies()
        .keys()
        .next()
        .unwrap()
}

fn assert_copy_target(game: &GameState, target: TargetRef) {
    let window = game.execution().window();
    let entry = window.entries().last().unwrap();
    assert_eq!(entry.id(), copy_id(game));
    assert!(matches!(entry.kind(), StackKind::SpellCopy { .. }));
    assert_eq!(entry.targets().targets()[0].choice().target, target);
}

#[test]
fn initialization_and_candidates_preserve_exact_outer_basis_without_mutating_input() {
    let expected_context = context();
    let candidate = initialize(GameId(10), expected_context.clone());
    assert_eq!(
        candidate.base(),
        &CandidateBase::Initialize {
            game: GameId(10),
            context: expected_context,
        }
    );
    let game = candidate.into_state();
    assert_eq!(game.basis().revision, 0);
    assert_eq!(game.basis().epoch, 0);
    assert_eq!(holder(&game), A);
    assert!(game.execution().revision() > game.basis().revision);
    let next = advance(&game, A, GameCommand::Pass);
    assert_eq!(holder(&next), B);
    reject(
        &next,
        input(&game, B, GameCommand::Pass),
        GameError::StaleRevision,
    );
    let mut future = input(&next, B, GameCommand::Pass);
    future.basis.revision += 1;
    reject(&next, future, GameError::StaleRevision);
}

#[test]
fn every_game_context_epoch_and_actor_boundary_rejects_without_effects() {
    let game = pending();
    let command = GameCommand::Reply {
        decision: game.pending_decision().unwrap().id(),
        choice: (CopyTargetChoice::Retain).into(),
    };
    let mut wrong_game = input(&game, B, command.clone());
    wrong_game.basis.game = GameId(11);
    reject(&game, wrong_game, GameError::WrongGame);
    let original = [
        "release-1",
        "rules-1",
        "catalogue-1",
        "programs-1",
        "profile-1",
    ];
    for changed in 0..original.len() {
        let mut fields = original;
        fields[changed] = "different";
        let mut request = input(&game, B, command.clone());
        request.basis.context = make_context(fields);
        reject(&game, request, GameError::WrongContext);
    }
    let mut wrong_epoch = input(&game, B, command.clone());
    wrong_epoch.basis.epoch += 1;
    reject(&game, wrong_epoch, GameError::WrongEpoch);
    reject(
        &game,
        input(&game, SeatId(99), command),
        GameError::UnknownActor,
    );
    let foreign = initialize(GameId(11), context()).into_state();
    reject(
        &foreign,
        input(&setup(), A, GameCommand::Pass),
        GameError::WrongGame,
    );
}

#[test]
fn pending_resolution_is_owned_exact_and_has_no_priority_window() {
    let game = pending();
    let saved = game.clone();
    let decision = game.pending_decision().unwrap();
    assert_eq!(decision.basis(), game.basis());
    assert_eq!(decision.chooser(), B);
    let RuleFrame::CopyTargets(frame) = decision.frame() else {
        panic!("expected copy frame")
    };
    let entries = game.execution().window().entries();
    assert_eq!(frame.entry(), entries[1].id());
    assert_eq!(frame.original(), entries[0].id());
    assert_eq!(frame.chooser(), B);
    assert_eq!(
        game.execution().window().phase(),
        WindowPhase::Resolving(frame.entry())
    );
    assert!(game.execution().window().spell_copies().is_empty());
    assert_eq!(zone(&game, 2), CardZone::Stack);
    assert_eq!(game.execution().mana(B, ManaType::Blue), 0.into());
    for actor in [A, B] {
        for command in [
            GameCommand::Pass,
            GameCommand::Cast {
                spell: object(&game, 3),
                target: TargetRef::Stack(frame.original()),
            },
        ] {
            reject(
                &game,
                input(&game, actor, command),
                GameError::DecisionRequired,
            );
        }
    }
    let resumed = reply(&saved, CopyTargetChoice::Retain);
    assert_eq!(resumed, reply(&game, CopyTargetChoice::Retain));
    assert_eq!(game, saved);
    assert!(resumed.pending_decision().is_none());
    assert_copy_target(&resumed, TargetRef::Card(object(&game, 5)));
}

#[test]
fn wrong_chooser_decision_and_revision_preserve_the_same_pending_choice() {
    let game = pending();
    let decision = game.pending_decision().unwrap().id();
    reject(
        &game,
        input(
            &game,
            A,
            GameCommand::Reply {
                decision,
                choice: (CopyTargetChoice::Retain).into(),
            },
        ),
        GameError::WrongChooser,
    );
    reject(
        &game,
        input(
            &game,
            B,
            GameCommand::Reply {
                decision: DecisionId(decision.0 + 1),
                choice: (CopyTargetChoice::Retain).into(),
            },
        ),
        GameError::WrongDecision,
    );
    for revision in [game.basis().revision - 1, game.basis().revision + 1] {
        let mut request = input(
            &game,
            B,
            GameCommand::Reply {
                decision,
                choice: (CopyTargetChoice::Retain).into(),
            },
        );
        request.basis.revision = revision;
        reject(&game, request, GameError::StaleRevision);
    }
    let resumed = reply(&game, CopyTargetChoice::Retain);
    assert_eq!(resumed.execution().window().spell_copies().len(), 1);
}

#[test]
fn accepted_reply_cannot_be_reused_or_replaced_with_refreshed_basis() {
    let game = pending();
    let decision = game.pending_decision().unwrap().id();
    let old_request = input(
        &game,
        B,
        GameCommand::Reply {
            decision,
            choice: (CopyTargetChoice::Retain).into(),
        },
    );
    let selected = TargetRef::Card(object(&game, 6));
    let accepted = reply(&game, CopyTargetChoice::Change(selected));
    reject(&accepted, old_request, GameError::StaleRevision);
    for choice in [CopyTargetChoice::Retain, CopyTargetChoice::Change(selected)] {
        reject(
            &accepted,
            input(
                &accepted,
                B,
                GameCommand::Reply {
                    decision,
                    choice: choice.into(),
                },
            ),
            GameError::NoDecision,
        );
    }
    assert_copy_target(&accepted, selected);
    assert_eq!(accepted.execution().window().spell_copies().len(), 1);
    assert_eq!(zone(&accepted, 2), CardZone::Graveyard(B));
    assert_eq!(accepted.execution().mana(B, ManaType::Blue), 0.into());
}

#[test]
fn a_later_pending_choice_does_not_accept_the_consumed_decision_identity() {
    let mut seeds = cards();
    seeds.push(instant(7, B, ManaType::Blue, 2, Instruction::CopySpell));
    let mut pools = mana();
    pools[2].amount = 4.into();
    let game = GameState::initialize(GameId(10), context(), vec![A, B], A, seeds, pools)
        .unwrap()
        .into_state();
    let target = object(&game, 5);
    let game = cast(game, A, 1, TargetRef::Card(target));
    let original = game.execution().window().entries()[0].id();
    let game = pass_round(cast(game, B, 2, TargetRef::Stack(original)));
    let first_decision = game.pending_decision().unwrap().id();
    let game = reply(&game, CopyTargetChoice::Retain);
    let game = pass_round(cast(game, B, 7, TargetRef::Stack(original)));
    assert_ne!(game.pending_decision().unwrap().id(), first_decision);
    reject(
        &game,
        input(
            &game,
            B,
            GameCommand::Reply {
                decision: first_decision,
                choice: (CopyTargetChoice::Retain).into(),
            },
        ),
        GameError::WrongDecision,
    );
    let game = reply(&game, CopyTargetChoice::Retain);
    assert_eq!(game.execution().window().spell_copies().len(), 2);
}

#[test]
fn retain_allows_departed_target_but_changed_targets_require_current_legality() {
    let (game, old) = missing_target();
    let decision = game.pending_decision().unwrap().id();
    let original = game.execution().window().entries()[0].id();
    for target in [
        TargetRef::Card(old),
        TargetRef::Card(object(&game, 5)),
        TargetRef::Player(A),
        TargetRef::Player(SeatId(99)),
        TargetRef::Stack(original),
    ] {
        reject(
            &game,
            input(
                &game,
                B,
                GameCommand::Reply {
                    decision,
                    choice: (CopyTargetChoice::Change(target)).into(),
                },
            ),
            GameError::Effect(EffectError::IllegalTarget),
        );
    }
    let retained = reply(&game, CopyTargetChoice::Retain);
    assert_copy_target(&retained, TargetRef::Card(old));
    let changed = reply(
        &game,
        CopyTargetChoice::Change(TargetRef::Card(object(&game, 6))),
    );
    assert_copy_target(&changed, TargetRef::Card(object(&game, 6)));
    let retained = pass_round(pass_round(retained));
    assert!(retained.execution().window().entries().is_empty());
    assert!(retained.execution().window().spell_copies().is_empty());
    assert_eq!(zone(&retained, 5), CardZone::Hand(B));
    assert_eq!(zone(&retained, 6), CardZone::Battlefield);
    assert_eq!(
        retained
            .execution()
            .creature(object(&retained, 6))
            .unwrap()
            .damage,
        0.into()
    );
    let changed = pass_round(pass_round(changed));
    assert_eq!(zone(&changed, 5), CardZone::Hand(B));
    assert_eq!(zone(&changed, 6), CardZone::Graveyard(A));
    assert!(changed.execution().window().entries().is_empty());
    assert_eq!(changed.execution().mana(A, ManaType::Red), 0.into());
    assert_eq!(changed.execution().mana(A, ManaType::Blue), 2.into());
    assert_eq!(changed.execution().mana(B, ManaType::Blue), 0.into());
}

#[test]
fn redirected_copy_is_independent_and_resolves_before_original_with_checks() {
    let game = copied();
    let window = game.execution().window();
    let copy = copy_id(&game);
    assert_ne!(copy, window.entries()[0].id());
    assert_eq!(window.cards().iter().count(), 6);
    assert_eq!(window.spell_copies()[&copy].owner, B);
    assert_eq!(window.spell_copies()[&copy].controller, Some(B));
    assert_eq!(game.execution().mana(B, ManaType::Blue), 0.into());
    assert_eq!(game.execution().mana(B, ManaType::Red), 0.into());
    let game = pass_round(game);
    assert_eq!(zone(&game, 5), CardZone::Battlefield);
    assert_eq!(zone(&game, 6), CardZone::Graveyard(A));
    assert!(game.execution().window().spell_copies().is_empty());
    assert_eq!(game.execution().window().entries().len(), 1);
    assert_eq!(holder(&game), A);
    let game = pass_round(game);
    assert_eq!(zone(&game, 5), CardZone::Graveyard(B));
    assert!(game.execution().window().entries().is_empty());
}

#[test]
fn removing_original_after_copy_creation_retains_exact_copied_payload() {
    let game = copied();
    let original = game.execution().window().entries()[0].id();
    let copy = copy_id(&game);
    let game = pass_round(cast(game, A, 3, TargetRef::Stack(original)));
    assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
    assert_eq!(game.execution().window().entries()[0].id(), copy);
    let restored_native_value = game.clone();
    let game = pass_round(restored_native_value);
    assert_eq!(zone(&game, 5), CardZone::Battlefield);
    assert_eq!(zone(&game, 6), CardZone::Graveyard(A));
    assert!(game.execution().window().entries().is_empty());
    assert!(game.execution().window().spell_copies().is_empty());
    assert_eq!(game.execution().mana(A, ManaType::Blue), 1.into());
    assert_eq!(game.execution().mana(B, ManaType::Blue), 0.into());
}

#[test]
fn copied_damage_quantity_survives_original_exit_without_reloading_source() {
    let mut seeds = cards();
    seeds[5].program = Program::VanillaCreature {
        power: 2.into(),
        toughness: 5.into(),
    };
    let game = GameState::initialize(GameId(10), context(), vec![A, B], A, seeds, mana())
        .unwrap()
        .into_state();
    let target = object(&game, 5);
    let game = cast(game, A, 1, TargetRef::Card(target));
    let original = game.execution().window().entries()[0].id();
    let game = pass_round(cast(game, B, 2, TargetRef::Stack(original)));
    let selected = object(&game, 6);
    let game = reply(&game, CopyTargetChoice::Change(TargetRef::Card(selected)));
    let game = pass_round(cast(game, A, 3, TargetRef::Stack(original)));
    assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
    let game = pass_round(game);
    let survivor = game.execution().creature(selected).unwrap();
    assert_eq!(survivor.toughness, 5.into());
    assert_eq!(survivor.damage, 3.into());
    assert_eq!(zone(&game, 5), CardZone::Battlefield);
    assert!(game.execution().window().entries().is_empty());
}

#[test]
fn removing_copy_leaves_original_and_never_refunds_paid_mana() {
    let game = copied();
    let copy = copy_id(&game);
    let game = pass_round(cast(game, A, 3, TargetRef::Stack(copy)));
    assert!(game.execution().window().spell_copies().is_empty());
    assert_eq!(game.execution().window().entries().len(), 1);
    assert_eq!(game.execution().mana(A, ManaType::Red), 0.into());
    assert_eq!(game.execution().mana(B, ManaType::Blue), 0.into());
    let game = pass_round(game);
    assert_eq!(zone(&game, 5), CardZone::Graveyard(B));
    assert_eq!(zone(&game, 6), CardZone::Battlefield);
    assert!(game.execution().window().entries().is_empty());
}

#[test]
fn target_departing_after_copy_creation_invalidates_both_payloads() {
    let game = pending();
    let old = object(&game, 5);
    let game = reply(&game, CopyTargetChoice::Retain);
    let game = pass_round(cast(game, A, 4, TargetRef::Card(old)));
    assert_ne!(old, object(&game, 5));
    let game = pass_round(pass_round(game));
    assert_eq!(zone(&game, 5), CardZone::Hand(B));
    assert_eq!(zone(&game, 6), CardZone::Battlefield);
    assert_eq!(
        game.execution().creature(object(&game, 6)).unwrap().damage,
        0.into()
    );
    assert!(game.execution().window().entries().is_empty());
    assert!(game.execution().window().spell_copies().is_empty());
}

#[test]
fn original_removed_before_copy_resolution_creates_neither_copy_nor_decision() {
    let game = stacked();
    let original = game.execution().window().entries()[0].id();
    let game = pass_round(cast(game, A, 3, TargetRef::Stack(original)));
    assert_eq!(zone(&game, 1), CardZone::Graveyard(A));
    assert!(game.pending_decision().is_none());
    let game = pass_round(game);
    assert_eq!(zone(&game, 2), CardZone::Graveyard(B));
    assert!(game.pending_decision().is_none());
    assert!(game.execution().window().entries().is_empty());
    assert!(game.execution().window().spell_copies().is_empty());
    assert_eq!(zone(&game, 5), CardZone::Battlefield);
    assert_eq!(zone(&game, 6), CardZone::Battlefield);
    reject(
        &game,
        input(
            &game,
            B,
            GameCommand::Reply {
                decision: DecisionId(1),
                choice: (CopyTargetChoice::Retain).into(),
            },
        ),
        GameError::NoDecision,
    );
}

#[test]
fn ordinary_damage_never_accepts_a_copy_choice_and_empty_pass_round_ends_step() {
    let game = setup();
    let target = object(&game, 5);
    let game = cast(game, A, 1, TargetRef::Card(target));
    reject(
        &game,
        input(
            &game,
            A,
            GameCommand::Reply {
                decision: DecisionId(1),
                choice: (CopyTargetChoice::Retain).into(),
            },
        ),
        GameError::NoDecision,
    );
    let game = pass_round(game);
    assert!(game.pending_decision().is_none());
    assert_eq!(zone(&game, 5), CardZone::Graveyard(B));
    let game = pass_round(game);
    assert_eq!(game.execution().window().phase(), WindowPhase::StepEnded);
}
