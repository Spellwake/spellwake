//! Ordinary declarations and damage through the exact native root. These literal
//! programs do not qualify keywords, complete cards or games.
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::{
    combat::{
        AssignmentOrigin, AttackDeclaration, BlockDeclaration, BlockStatus, CombatDamageAssignment,
        CombatDecision, CombatDeclarationRecord, CombatError, CombatRules, DeclarationOrigin,
    },
    damage::{
        DamageAllocation, DamageError, DamageKind, DamageOrigin, DamageRecipient, DamageSourceId,
    },
    effects::{DamageDecision, EffectError, Instruction, Program, SeedCard},
    game::{
        DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState, NativeCandidate, OpeningSetup, RuleFrame,
    },
    mana::{ManaCost, ManaType},
    object_identity::{CardId, GameObject, ObjectDestination, ObjectZone},
    opening::MulliganChoice,
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStage, TurnStart, TurnWork},
};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);
const C: SeatId = SeatId(90);
const D: SeatId = SeatId(2);

fn context() -> GameContext {
    GameContext::new(
        "declarations".into(),
        "cr-20260925".into(),
        "authored".into(),
        "ordinary-creatures".into(),
        "explicit-ffa".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn creature(id: u64, player: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: player,
        destination,
        program: Program::creature(
            Some(ManaCost::free()),
            2.into(),
            4.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}
fn battlefield(id: u64, player: SeatId) -> SeedCard {
    creature(
        id,
        player,
        ObjectDestination::Battlefield { controller: player },
    )
}
fn instant(id: u64, player: SeatId, instruction: Instruction) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: player,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
        ),
    }
}
fn cards() -> Vec<SeedCard> {
    vec![
        battlefield(1, A),
        battlefield(2, A),
        battlefield(3, B),
        battlefield(4, B),
        SeedCard {
            card: CardId(20),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::artifact(None, vec![]),
        },
        instant(30, A, Instruction::ReturnToOwnersHand),
        instant(31, B, Instruction::ReturnToOwnersHand),
        instant(32, A, Instruction::Damage { amount: 4.into() }),
        creature(40, A, ObjectDestination::Hand),
    ]
}
fn setup(seats: Vec<SeatId>, cards: Vec<SeedCard>, step: Step) -> NativeCandidate {
    setup_with_life(seats, cards, step, 20)
}
fn setup_with_life(
    seats: Vec<SeatId>,
    mut cards: Vec<SeedCard>,
    step: Step,
    life: i64,
) -> NativeCandidate {
    for player in &seats {
        cards.push(creature(
            1000 + u64::from(player.0),
            *player,
            ObjectDestination::LibraryBottom,
        ));
    }
    GameState::initialize_scheduled(
        GameId(508),
        context(),
        {
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                seats,
                A,
                cards,
                initial_battlefield_order,
                vec![],
                life.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        },
        TurnStart { number: 1, step },
    )
    .unwrap()
}
fn object(game: &GameState, id: u64) -> &GameObject {
    let cards = game.execution().window().cards();
    cards
        .get(cards.current_identity(CardId(id)).unwrap())
        .unwrap()
}
fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected actual ready priority")
    };
    holder
}
fn input(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    }
}
fn act(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(input(game, actor, command)).unwrap()
}
fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    let pending = game.pending_decision().unwrap();
    act(
        game,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}
fn round(game: &GameState) -> NativeCandidate {
    let mut state = game.clone();
    for _ in 1..game.execution().window().participants().len() {
        state = act(&state, holder(&state), GameCommand::Pass).into_state();
    }
    act(&state, holder(&state), GameCommand::Pass)
}
fn attackers(game: &GameState) -> GameState {
    let mut next = game.clone();
    for _ in 0..32 {
        if next.turn().unwrap().position().step == Step::DeclareAttackers {
            return next;
        }
        next = act(&next, holder(&next), GameCommand::Pass).into_state();
    }
    panic!("declaration boundary was not reached")
}
fn declared(game: &GameState, selected: &[(u64, SeatId)]) -> NativeCandidate {
    reply(
        game,
        DecisionReply::DeclareAttackers(
            selected
                .iter()
                .map(|&(id, defender)| AttackDeclaration {
                    attacker: object(game, id).id(),
                    defender,
                })
                .collect(),
        ),
    )
}
fn blocked(game: &GameState, selected: &[(u64, u64)]) -> NativeCandidate {
    reply(
        game,
        DecisionReply::DeclareBlockers(
            selected
                .iter()
                .map(|&(blocker, attacker)| BlockDeclaration {
                    blocker: object(game, blocker).id(),
                    attacker: object(game, attacker).id(),
                })
                .collect(),
        ),
    )
}
fn reject(game: &GameState, incoming: GameInput, expected: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(incoming), Err(expected));
    assert_eq!(game, &saved);
}

fn assignment_pending(shields: &[i64], unblocked_attacker: bool) -> GameState {
    let mut seeds = cards();
    for (index, &amount) in shields.iter().enumerate() {
        seeds.push(instant(
            50 + index as u64,
            A,
            Instruction::CreatePreventionShield {
                amount: amount.into(),
            },
        ));
    }
    let mut game = setup(vec![A, B], seeds, Step::Untap).into_state();
    for index in 0..shields.len() {
        let cast = act(
            &game,
            A,
            GameCommand::Cast {
                spell: object(&game, 50 + index as u64).id(),
                target: TargetRef::Object(object(&game, 1).id()),
            },
        );
        game = round(cast.state()).into_state();
    }
    let ready = attackers(&game);
    let selected = if unblocked_attacker {
        vec![(1, B), (2, B)]
    } else {
        vec![(1, B)]
    };
    let attacking = declared(&ready, &selected);
    let blocking = round(attacking.state());
    let blocks = blocked(blocking.state(), &[(3, 1), (4, 1)]);
    round(blocks.state()).into_state()
}

fn free_assignment(game: &GameState, unblocked_attacker: bool) -> Vec<CombatDamageAssignment> {
    // The first blocker receives zero despite being first in the current domain.
    // Current ordinary assignment has no lethal-first or retired order constraint.
    let mut selected = vec![CombatDamageAssignment {
        source: object(game, 1).id(),
        target: DamageRecipient::Creature(object(game, 4).id()),
        amount: 2.into(),
    }];
    if unblocked_attacker {
        selected.push(CombatDamageAssignment {
            source: object(game, 2).id(),
            target: DamageRecipient::Player(B),
            amount: 2.into(),
        });
    }
    selected
}

#[test]
fn actual_damage_assignment_is_free_sparse_whole_actor_input_with_one_simultaneous_result() {
    let game = assignment_pending(&[], true);
    let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
        game.pending_decision().unwrap().frame()
    else {
        panic!("expected an actual assignment decision")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.position(), game.turn().unwrap().position());
    assert_eq!(frame.domains().len(), 2);
    assert_eq!(frame.domains()[0].power(), &2.into());
    assert_eq!(frame.domains()[0].recipients().len(), 2);
    assert_eq!(
        frame.domains()[1].recipients(),
        &[DamageRecipient::Player(B)]
    );
    let valid = free_assignment(&game, true);
    let mut invalid = Vec::new();
    invalid.push((vec![valid[0].clone()], CombatError::IncorrectDamageTotal));
    let mut rows = valid.clone();
    rows[0].amount = 1.into();
    invalid.push((rows, CombatError::IncorrectDamageTotal));
    let mut rows = valid.clone();
    rows.push(valid[0].clone());
    invalid.push((rows, CombatError::DuplicateAssignment));
    let mut rows = valid.clone();
    rows[0].amount = 0.into();
    invalid.push((rows, CombatError::InvalidDamageAmount));
    let mut rows = valid.clone();
    rows[0].amount = (-1).into();
    invalid.push((rows, CombatError::InvalidDamageAmount));
    let mut rows = valid.clone();
    rows[0].source = object(&game, 20).id();
    invalid.push((rows, CombatError::IllegalDamageSource));
    let mut rows = valid.clone();
    rows[0].target = DamageRecipient::Player(B);
    invalid.push((rows, CombatError::IllegalDamageRecipient));
    for (rows, error) in invalid {
        let pending = game.pending_decision().unwrap();
        reject(
            &game,
            input(
                &game,
                A,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::AssignCombatDamage(rows),
                },
            ),
            GameError::Effect(EffectError::Combat(error)),
        );
    }
    reject(
        &game,
        input(&game, A, GameCommand::Pass),
        GameError::TurnWorkRequired(TurnWork::CombatDamage { additional: false }),
    );
    let source = object(&game, 1).id();
    let completed = reply(&game, DecisionReply::AssignCombatDamage(valid));
    let journal = completed.damage_journal();
    assert_eq!(journal.batches.len(), 1);
    assert_eq!(journal.batches[0].kind, DamageKind::Combat);
    assert_eq!(
        journal.batches[0].origin,
        DamageOrigin::Combat {
            position: game.turn().unwrap().position()
        }
    );
    assert_eq!(journal.attempts.len(), 4);
    assert_eq!(journal.applied.len(), 4);
    assert_eq!(journal.completions.len(), 4);
    assert!(
        journal
            .applied
            .iter()
            .any(|fact| fact.source.id() == DamageSourceId::Object(source))
    );
    assert_eq!(completed.combat_assignments().len(), 2);
    assert_eq!(
        completed.combat_assignments()[0].origin,
        AssignmentOrigin::PlayerReply
    );
    assert_eq!(
        completed.combat_assignments()[1].origin,
        AssignmentOrigin::Forced
    );
    assert_eq!(completed.combat_assignments()[1].player, B);
    let state = completed.state();
    assert_eq!(state.execution().life_total(B), Some(&18.into()));
    assert_eq!(object(state, 1).zone(), ObjectZone::Graveyard(A));
    assert!(state.execution().creature(source).unwrap().is_none());
    assert_eq!(
        state
            .execution()
            .creature(object(state, 3).id())
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
    assert_eq!(
        state
            .execution()
            .creature(object(state, 4).id())
            .unwrap()
            .unwrap()
            .damage,
        2.into()
    );
    assert_eq!(state.turn().unwrap().stage(), TurnStage::Priority);
    assert_eq!(holder(state), A);
    let later = act(state, A, GameCommand::Pass);
    assert!(later.damage_journal().batches.is_empty());
    assert!(later.damage_journal().applied.is_empty());
    assert!(later.combat_assignments().is_empty());
}

#[test]
fn one_actual_combat_shield_requires_exact_interior_allocation_and_holds_all_results_pending() {
    let assigning = assignment_pending(&[3], false);
    let source = object(&assigning, 1).id();
    let blocked = reply(
        &assigning,
        DecisionReply::AssignCombatDamage(free_assignment(&assigning, false)),
    );
    let game = blocked.state();
    let RuleFrame::Damage(DamageDecision::Allocation(frame)) =
        game.pending_decision().unwrap().frame()
    else {
        panic!("one finite shield still has a real allocation decision")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.capacity(), &3.into());
    assert_eq!(frame.required(), &3.into());
    assert_eq!(frame.eligible().len(), 2);
    assert_eq!(
        *frame.origin(),
        DamageOrigin::Combat {
            position: game.turn().unwrap().position()
        }
    );
    assert_eq!(blocked.damage_journal().batches.len(), 1);
    assert!(blocked.damage_journal().preventions.is_empty());
    assert!(blocked.damage_journal().applied.is_empty());
    for id in [1, 3, 4] {
        assert_eq!(
            game.execution()
                .creature(object(game, id).id())
                .unwrap()
                .unwrap()
                .damage,
            0.into()
        );
    }
    let one = frame.eligible()[0].attempt();
    let two = frame.eligible()[1].attempt();
    let ineligible = game
        .execution()
        .pending_damage()
        .unwrap()
        .components()
        .find(|component| component.attempt().source.id() == DamageSourceId::Object(source))
        .unwrap()
        .attempt()
        .id;
    for allocations in [
        vec![DamageAllocation {
            attempt: one,
            amount: 1.into(),
        }],
        vec![DamageAllocation {
            attempt: one,
            amount: 3.into(),
        }],
        vec![
            DamageAllocation {
                attempt: one,
                amount: 1.into(),
            },
            DamageAllocation {
                attempt: one,
                amount: 2.into(),
            },
        ],
        vec![
            DamageAllocation {
                attempt: one,
                amount: 0.into(),
            },
            DamageAllocation {
                attempt: two,
                amount: 3.into(),
            },
        ],
        vec![
            DamageAllocation {
                attempt: ineligible,
                amount: 1.into(),
            },
            DamageAllocation {
                attempt: two,
                amount: 2.into(),
            },
        ],
    ] {
        reject(
            game,
            input(
                game,
                A,
                GameCommand::Reply {
                    decision: game.pending_decision().unwrap().id(),
                    choice: DecisionReply::AllocatePrevention(allocations),
                },
            ),
            GameError::Effect(EffectError::Damage(DamageError::InvalidAllocation)),
        );
    }
    let mut states = Vec::new();
    for (left, right) in [(1, 2), (2, 1)] {
        let completed = reply(
            game,
            DecisionReply::AllocatePrevention(vec![
                DamageAllocation {
                    attempt: one,
                    amount: left.into(),
                },
                DamageAllocation {
                    attempt: two,
                    amount: right.into(),
                },
            ]),
        );
        assert!(completed.damage_journal().batches.is_empty());
        assert!(completed.damage_journal().attempts.is_empty());
        assert!(completed.combat_assignments().is_empty());
        let application = &completed.damage_journal().preventions[0];
        assert_eq!(application.amount, 3.into());
        assert_eq!(application.damage_remaining[&one], (2 - left).into());
        assert_eq!(application.damage_remaining[&two], (2 - right).into());
        assert_eq!(
            completed
                .state()
                .execution()
                .creature(source)
                .unwrap()
                .unwrap()
                .damage,
            1.into()
        );
        assert_eq!(
            completed
                .state()
                .execution()
                .creature(object(game, 4).id())
                .unwrap()
                .unwrap()
                .damage,
            2.into()
        );
        assert!(
            completed
                .state()
                .execution()
                .prevention_shields()
                .next()
                .is_none()
        );
        assert_eq!(
            completed.state().turn().unwrap().stage(),
            TurnStage::Priority
        );
        states.push(completed.into_state());
    }
    assert_eq!(states[0], states[1]);
}

#[test]
fn unresolved_simultaneous_shield_scope_is_an_error_without_accepted_assignment_or_fake_completion()
{
    let game = assignment_pending(&[3, 5], false);
    let pending = game.pending_decision().unwrap();
    reject(
        &game,
        input(
            &game,
            A,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::AssignCombatDamage(free_assignment(&game, false)),
            },
        ),
        GameError::Effect(EffectError::Damage(
            DamageError::UnsupportedSimultaneousScope,
        )),
    );
    assert!(game.execution().pending_damage().is_none());
    assert_eq!(game.execution().prevention_shields().count(), 2);
    assert_eq!(
        game.pending_turn_work(),
        Some(TurnWork::CombatDamage { additional: false })
    );
    for id in [1, 3, 4] {
        assert_eq!(
            game.execution()
                .creature(object(&game, id).id())
                .unwrap()
                .unwrap()
                .damage,
            0.into()
        );
    }
}

#[test]
fn zero_or_negative_current_attack_power_completes_actual_work_without_damage_occurrence() {
    for power in [0, -2] {
        let mut seeds = cards();
        let spellwake_engine::program::ProgramKind::Permanent(permanent) =
            &mut seeds[0].program.kind
        else {
            panic!("creature fixture")
        };
        permanent.printed_pt.as_mut().unwrap().power =
            spellwake_engine::characteristics::PrintedStat::Number(power.into());
        let ready = attackers(setup(vec![A, B], seeds, Step::Untap).state());
        let attacking = declared(&ready, &[(1, B)]);
        let blocking = round(attacking.state());
        let unblocked = blocked(blocking.state(), &[]);
        let damage = round(unblocked.state());
        assert_eq!(
            damage.state().turn().unwrap().position().step,
            Step::CombatDamage
        );
        assert_eq!(damage.state().turn().unwrap().stage(), TurnStage::Priority);
        assert!(damage.damage_journal().batches.is_empty());
        assert!(damage.combat_assignments().is_empty());
        assert_eq!(damage.state().execution().life_total(B), Some(&20.into()));
    }
}

#[test]
fn actual_turn_start_drives_current_domains_into_one_exact_work_frame() {
    let initial = setup(vec![A, B], cards(), Step::Untap).into_state();
    let game = attackers(&initial);
    let pending = game.pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) = pending.frame() else {
        panic!("expected attackers")
    };
    assert_eq!(pending.basis(), game.basis());
    assert_eq!(pending.chooser(), A);
    assert_eq!(frame.revision(), game.execution().revision());
    assert_eq!(
        frame.eligible_attackers(),
        &[object(&game, 1).id(), object(&game, 2).id()]
    );
    assert_eq!(frame.defenders(), &[B]);
    assert_eq!(game.pending_turn_work(), Some(TurnWork::DeclareAttackers));
    assert_eq!(game.execution().window().phase(), WindowPhase::TurnWork);
    assert!(game.execution().combat().is_none());
    assert!(game.execution().window().entries().is_empty());
    assert_eq!(object(&game, 1).tapped(), Some(false));
}

#[test]
fn prepared_attackers_do_not_invent_control_age_and_forced_empty_reaches_real_exit() {
    let initial = setup(vec![A, B], cards(), Step::DeclareAttackers);
    let game = initial.state();
    assert!(game.pending_decision().is_none());
    assert_eq!(holder(game), A);
    assert_eq!(
        object(game, 1).has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert!(
        matches!(initial.combat_declarations(), [CombatDeclarationRecord::Attackers { player: A, origin: DeclarationOrigin::ForcedEmpty, attackers, taps }] if attackers.is_empty() && taps.is_empty())
    );
    let original = game.execution().window().cards().clone();
    let end = round(game);
    assert_eq!(end.state().turn().unwrap().position().step, Step::EndCombat);
    assert!(end.state().execution().combat().is_some());
    assert!(end.combat_declarations().is_empty());
    let main = round(end.state());
    assert_eq!(
        main.state().turn().unwrap().position().step,
        Step::PostcombatMain
    );
    assert!(main.state().execution().combat().is_none());
    assert_eq!(main.state().execution().window().cards(), &original);
}

#[test]
fn explicit_empty_choice_has_player_origin_and_never_repeats_on_later_inputs() {
    let game = attackers(setup(vec![A, B], cards(), Step::Untap).state());
    let registry = game.execution().window().cards().clone();
    let accepted = declared(&game, &[]);
    assert!(
        matches!(accepted.combat_declarations(), [CombatDeclarationRecord::Attackers { origin: DeclarationOrigin::PlayerReply, attackers, taps, .. }] if attackers.is_empty() && taps.is_empty())
    );
    assert_eq!(accepted.state().execution().window().cards(), &registry);
    assert_eq!(holder(accepted.state()), A);
    assert!(
        !accepted
            .state()
            .execution()
            .combat()
            .unwrap()
            .had_attackers()
    );
    let passed = act(accepted.state(), A, GameCommand::Pass);
    assert!(passed.combat_declarations().is_empty());
    assert!(passed.mana_payment().is_none());
    assert!(passed.mana_production().is_none());
    assert!(passed.permanent_entry().is_none());
}

#[test]
fn accepted_attackers_tap_physical_incarnations_and_emit_only_real_snapshots() {
    let game = attackers(setup(vec![A, B], cards(), Step::Untap).state());
    let first = object(&game, 1).clone();
    let second = object(&game, 2).clone();
    let revision = game.execution().window().cards().revision();
    let accepted = declared(&game, &[(2, B), (1, B)]);
    let [
        CombatDeclarationRecord::Attackers {
            player,
            origin,
            attackers,
            taps,
        },
    ] = accepted.combat_declarations()
    else {
        panic!("missing attack record")
    };
    assert_eq!((*player, *origin), (A, DeclarationOrigin::PlayerReply));
    assert_eq!(
        attackers,
        &vec![
            AttackDeclaration {
                attacker: first.id(),
                defender: B
            },
            AttackDeclaration {
                attacker: second.id(),
                defender: B
            }
        ]
    );
    assert_eq!(taps.len(), 2);
    for (tap, before) in taps.iter().zip([first, second]) {
        assert_eq!(tap.before, before);
        assert_eq!(
            &tap.after,
            object(
                accepted.state(),
                tap.after.card_id().expect("physical fixture card").0
            )
        );
        assert_eq!(tap.after.id(), before.id());
        assert_eq!(tap.after.owner(), before.owner());
        assert_eq!(tap.after.controller(), before.controller());
        assert_eq!(tap.after.control_epoch(), before.control_epoch());
        assert_eq!(
            tap.after.has_continuous_control_since_turn_start(),
            Some(true)
        );
        assert_eq!(tap.after.tapped(), Some(true));
    }
    assert_eq!(
        accepted.state().execution().window().cards().revision(),
        revision + 2
    );
    assert_eq!(holder(accepted.state()), A);
    assert!(accepted.state().execution().window().entries().is_empty());
    assert!(accepted.mana_payment().is_none());
    assert!(accepted.ability_activation().is_none());
    assert_eq!(
        accepted
            .state()
            .execution()
            .combat()
            .unwrap()
            .attacker(object(&game, 1).id())
            .unwrap()
            .block_status(),
        BlockStatus::NotDeclared
    );
}

#[test]
fn work_gate_preserves_exact_basis_token_chooser_and_kind_errors() {
    let game = attackers(setup(vec![A, B], cards(), Step::Untap).state());
    let pending = game.pending_decision().unwrap();
    let base = input(
        &game,
        A,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::DeclareAttackers(vec![]),
        },
    );
    for (incoming, error) in [
        (
            {
                let mut i = base.clone();
                i.basis.game = GameId(999);
                i
            },
            GameError::WrongGame,
        ),
        (
            {
                let mut i = base.clone();
                i.basis.context = GameContext::new(
                    "different".into(),
                    "r".into(),
                    "c".into(),
                    "p".into(),
                    "f".into(),
                    test_vocabulary().id(),
                    spellwake_contracts::names::NameRules::literal_names().id(),
                )
                .unwrap();
                i
            },
            GameError::WrongContext,
        ),
        (
            {
                let mut i = base.clone();
                i.basis.epoch += 1;
                i
            },
            GameError::WrongEpoch,
        ),
        (
            {
                let mut i = base.clone();
                i.basis.revision += 1;
                i
            },
            GameError::StaleRevision,
        ),
        (
            {
                let mut i = base.clone();
                i.actor = SeatId(999);
                i
            },
            GameError::UnknownActor,
        ),
        (
            {
                let mut i = base.clone();
                i.actor = B;
                i
            },
            GameError::WrongChooser,
        ),
        (
            input(
                &game,
                A,
                GameCommand::Reply {
                    decision: DecisionId(pending.id().0 + 1),
                    choice: DecisionReply::DeclareAttackers(vec![]),
                },
            ),
            GameError::WrongDecision,
        ),
        (
            input(
                &game,
                A,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::ManaColor(ManaType::Blue),
                },
            ),
            GameError::Effect(EffectError::WrongDecisionKind),
        ),
        (
            input(
                &game,
                A,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::DeclareBlockers(vec![]),
                },
            ),
            GameError::Effect(EffectError::WrongDecisionKind),
        ),
    ] {
        reject(&game, incoming, error);
    }
    for command in [
        GameCommand::Pass,
        GameCommand::Cast {
            spell: object(&game, 30).id(),
            target: TargetRef::Object(object(&game, 3).id()),
        },
    ] {
        reject(
            &game,
            input(&game, A, command),
            GameError::TurnWorkRequired(TurnWork::DeclareAttackers),
        );
    }
}

#[test]
fn illegal_whole_attack_declarations_preserve_cards_and_the_original_frame() {
    let game = attackers(setup(vec![A, B], cards(), Step::Untap).state());
    let pending = game.pending_decision().unwrap();
    let good = AttackDeclaration {
        attacker: object(&game, 1).id(),
        defender: B,
    };
    let choices = [
        vec![good, good],
        vec![AttackDeclaration {
            defender: A,
            ..good
        }],
        vec![AttackDeclaration {
            defender: SeatId(999),
            ..good
        }],
        vec![
            good,
            AttackDeclaration {
                attacker: object(&game, 3).id(),
                defender: B,
            },
        ],
        vec![
            good,
            AttackDeclaration {
                attacker: object(&game, 20).id(),
                defender: B,
            },
        ],
        vec![
            good,
            AttackDeclaration {
                attacker: object(&game, 40).id(),
                defender: B,
            },
        ],
    ];
    for choice in choices {
        let saved = game.clone();
        assert!(
            game.advance(input(
                &game,
                A,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::DeclareAttackers(choice)
                }
            ))
            .is_err()
        );
        assert_eq!(game, saved);
        assert_eq!(object(&game, 1).tapped(), Some(false));
    }
    assert_eq!(
        declared(&game, &[(1, B)])
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .count(),
        1
    );
}

#[test]
fn newly_cast_creatures_cannot_attack_but_fresh_defenders_can_block_many_to_one() {
    let initial = setup(vec![A, B], cards(), Step::Untap).into_state();
    let main = round(&initial).into_state();
    assert_eq!(main.turn().unwrap().position().step, Step::PrecombatMain);
    let cast = act(
        &main,
        A,
        GameCommand::CastUntargeted {
            spell: object(&main, 40).id(),
            payment: None,
        },
    )
    .into_state();
    let resolved = round(&cast).into_state();
    let game = attackers(&resolved);
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) =
        game.pending_decision().unwrap().frame()
    else {
        panic!("expected attackers")
    };
    assert!(!frame.eligible_attackers().contains(&object(&game, 40).id()));
    let saved = game.clone();
    let pending = game.pending_decision().unwrap();
    assert!(
        game.advance(input(
            &game,
            A,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                    attacker: object(&game, 40).id(),
                    defender: B
                }]),
            }
        ))
        .is_err()
    );
    assert_eq!(game, saved);
    let attacking = declared(&game, &[(1, B)]);
    let blockers = round(attacking.state());
    let blocking = blockers.state();
    let RuleFrame::Combat(CombatDecision::Blockers(frame)) =
        blocking.pending_decision().unwrap().frame()
    else {
        panic!("expected blockers")
    };
    assert_eq!(frame.chooser(), B);
    assert_eq!(
        frame.eligible_blockers(),
        &[object(blocking, 3).id(), object(blocking, 4).id()]
    );
    assert_eq!(
        object(blocking, 3).has_continuous_control_since_turn_start(),
        Some(false)
    );
    let registry = blocking.execution().window().cards().clone();
    let accepted = blocked(blocking, &[(4, 1), (3, 1)]);
    assert_eq!(accepted.state().execution().window().cards(), &registry);
    let combat = accepted.state().execution().combat().unwrap();
    assert_eq!(combat.blockers().count(), 2);
    assert_eq!(
        combat
            .attacker(object(blocking, 1).id())
            .unwrap()
            .block_status(),
        BlockStatus::Blocked
    );
    assert!(
        combat
            .blockers()
            .all(|b| b.attacker() == Some(object(blocking, 1).id()))
    );
    assert_eq!(holder(accepted.state()), A);
    let assignments = round(accepted.state());
    assert_eq!(
        assignments.state().pending_turn_work(),
        Some(TurnWork::CombatDamage { additional: false })
    );
    let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
        assignments.state().pending_decision().unwrap().frame()
    else {
        panic!("multiple blockers require an actual assignment reply")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.domains().len(), 1);
    assert_eq!(frame.domains()[0].source(), object(blocking, 1).id());
    assert_eq!(frame.domains()[0].recipients().len(), 2);
    assert!(assignments.damage_journal().attempts.is_empty());
}

#[test]
fn three_player_attacking_turn_owner_can_time_out_before_and_after_attacker_declaration() {
    let ready = attackers(setup(vec![A, B, C], cards(), Step::Untap).state());
    let attacking = declared(&ready, &[(1, B)]).into_state();
    for before in [&ready, &attacking] {
        let timeout = before.expire_response(before.basis(), A).unwrap();
        assert_eq!(before.expire_response(before.basis(), A).unwrap(), timeout);
        let mut next = timeout.into_state();
        assert_eq!(next.execution().window().participants(), &[B, C]);
        assert_eq!(next.execution().window().active_player(), None);
        for _ in 0..60 {
            if next.execution().window().active_player() == Some(B) {
                break;
            }
            assert!(next.pending_decision().is_none());
            let owner = holder(&next);
            assert_ne!(owner, A);
            next = next
                .advance(input(&next, owner, GameCommand::Pass))
                .unwrap()
                .into_state();
        }
        assert_eq!(next.execution().window().active_player(), Some(B));
        assert_eq!(next.execution().life_total(B), Some(&20.into()));
        assert_eq!(before.execution().window().participants(), &[A, B, C]);
    }
}

fn timeout_after_first_graveyard_order(seats: Vec<SeatId>) -> NativeCandidate {
    let seeds = [(1, A), (2, A), (3, A), (4, B), (5, B)]
        .into_iter()
        .map(|(id, seat)| SeedCard {
            program: Program::creature(None, 2.into(), 2.into(), Default::default()),
            ..battlefield(id, seat)
        })
        .collect();
    let start = setup_with_life(seats, seeds, Step::Untap, 2);
    let attack = declared(&attackers(start.state()), &[(1, B), (2, B), (3, C)]);
    let block = round(attack.state());
    let blocked = blocked(block.state(), &[(4, 1), (5, 2)]);
    let damage = round(blocked.state());
    let first = damage.state();
    let chosen_order = [2, 1].map(|id| object(first, id).id());
    assert_eq!(first.pending_decision().unwrap().chooser(), A);
    let ordered = reply(first, DecisionReply::OrderGraveyard(chosen_order.to_vec()));
    let before = ordered.state();
    assert_eq!(before.pending_decision().unwrap().chooser(), B);
    assert_eq!(before.execution().life_total(C), Some(&0.into()));
    assert!(before.conclusion().is_none());

    let timeout = before.expire_response(before.basis(), B).unwrap();
    assert_eq!(before.expire_response(before.basis(), B).unwrap(), timeout);
    let next = timeout.state();
    let graveyard = next.execution().window().cards().graveyard(A);
    assert_eq!(graveyard, [2, 1].map(|id| object(next, id).id()));
    assert!(before.execution().window().cards().graveyard(A).is_empty());
    timeout
}

#[test]
fn graveyard_order_timeout_settles_the_other_defenders_loss() {
    let timeout = timeout_after_first_graveyard_order(vec![A, B, C]);
    let next = timeout.state();
    assert_eq!(
        next.outcome(),
        Some(spellwake_engine::draw::GameOutcome::Winner(A))
    );
    assert_eq!(
        next.conclusion().unwrap().cause(),
        &spellwake_engine::game::GameConclusionCause::CheckedLossesAfterDepartures {
            players: [C].into(),
            earlier_departures: [B].into(),
        }
    );
}

#[test]
fn graveyard_order_timeout_leaves_two_survivors_acting_after_the_other_defender_loses() {
    let timeout = timeout_after_first_graveyard_order(vec![A, B, C, D]);
    let next = timeout.state();
    assert!(next.conclusion().is_none());
    assert_eq!(next.execution().window().participants(), &[A, D]);
    let departures: Vec<_> = timeout
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            spellwake_engine::occurrence::RulesRecord::PlayersDeparted { players, .. } => {
                Some(players.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(departures, [[B].into(), [C].into()]);
    let first = holder(next);
    let after = act(next, first, GameCommand::Pass);
    assert_ne!(holder(after.state()), first);
}

#[test]
fn static_control_survives_a_bystander_but_ends_when_the_source_owner_leaves() {
    use spellwake_engine::characteristics::{
        CardType, PermanentSelector, StaticAbilityId, StaticContinuousAbility, StaticPart,
        StaticParts,
    };

    let mut source = Program::artifact(None, vec![]);
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(1),
        selector: PermanentSelector {
            required_types: [CardType::Creature].into(),
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ControlSourceController]),
    });
    let start = setup(
        vec![A, B, C],
        vec![
            SeedCard {
                program: source,
                ..battlefield(1, A)
            },
            battlefield(2, C),
        ],
        Step::PrecombatMain,
    );
    let before = start.state();
    let original = object(before, 2);
    assert_eq!(original.controller(), Some(A));
    let bystander = act(before, B, GameCommand::Concede);
    assert_eq!(object(bystander.state(), 2), original);
    let expired = before.expire_response(before.basis(), A).unwrap();
    let returned = object(expired.state(), 2);
    assert_eq!(returned.id(), original.id());
    assert_eq!(returned.controller(), Some(C));
    assert_eq!(returned.zone(), ObjectZone::Battlefield);
    assert_ne!(returned.control_epoch(), original.control_epoch());
}

#[test]
fn combat_priority_bystander_can_concede_and_survivors_finish_the_attack() {
    let game = attackers(setup(vec![A, B, C, D], cards(), Step::Untap).state());
    let attacking = declared(&game, &[(1, B)]).into_state();
    let before = attacking.execution().combat().unwrap().clone();
    let leaving = input(&attacking, D, GameCommand::Concede);
    let accepted = attacking.advance(leaving.clone()).unwrap();
    assert_eq!(attacking.advance(leaving).unwrap(), accepted);
    let next = accepted.state();
    assert_eq!(next.execution().combat(), Some(&before));
    assert_eq!(next.execution().window().participants(), &[A, B, C]);
    assert_eq!(holder(next), A);
    let block = round(next);
    assert_eq!(block.state().pending_decision().unwrap().chooser(), B);
    let unblocked = blocked(block.state(), &[]);
    let damage = round(unblocked.state());
    assert_eq!(
        damage.state().turn().unwrap().position().step,
        Step::CombatDamage
    );
    assert_eq!(damage.state().execution().life_total(B), Some(&18.into()));
    assert_eq!(attacking.execution().combat(), Some(&before));
}

#[test]
fn departing_defender_skips_only_their_block_choice_and_preserves_other_damage() {
    let mut seeds = cards();
    seeds.push(battlefield(5, C));
    let game = attackers(setup(vec![A, B, C, D], seeds, Step::Untap).state());
    let attacking = declared(&game, &[(1, B), (2, C)]).into_state();
    let first = round(&attacking).into_state();
    assert_eq!(first.pending_decision().unwrap().chooser(), B);
    for departure in [
        attacking
            .advance(input(&attacking, B, GameCommand::Concede))
            .unwrap(),
        first.expire_response(first.basis(), B).unwrap(),
    ] {
        assert!(departure.combat_declarations().is_empty());
        let mut next = departure.into_state();
        if next.pending_decision().is_none() {
            next = round(&next).into_state();
        }
        assert_eq!(next.pending_decision().unwrap().chooser(), C);
        let after = blocked(&next, &[]);
        assert_eq!(
            after
                .state()
                .execution()
                .combat()
                .unwrap()
                .attacker(object(&game, 1).id())
                .unwrap()
                .block_status(),
            BlockStatus::Unblocked
        );
        let damage = round(after.state());
        assert_eq!(damage.state().execution().life_total(C), Some(&18.into()));
        assert_eq!(
            damage.state().execution().life_total(B),
            next.execution().life_total(B)
        );
    }
    let second = blocked(&first, &[(3, 1)]).into_state();
    let departure = second.expire_response(second.basis(), C).unwrap();
    assert!(departure.combat_declarations().is_empty());
    assert!(departure.state().pending_decision().is_none());
    let combat = departure.state().execution().combat().unwrap();
    assert_eq!(combat.blockers().count(), 1);
    assert_eq!(
        combat
            .attacker(object(&game, 1).id())
            .unwrap()
            .block_status(),
        BlockStatus::Blocked
    );
    let damage = round(departure.state());
    assert_eq!(damage.state().execution().life_total(B), Some(&20.into()));
    assert_eq!(first.execution().window().participants(), &[A, B, C, D]);
}

#[test]
fn departure_during_damage_assignment_preserves_surviving_attacks() {
    let game = attackers(setup(vec![A, B, C, D], cards(), Step::Untap).state());
    let attacks = declared(&game, &[(1, B), (2, C)]);
    let block = round(attacks.state());
    let blocks = blocked(block.state(), &[(3, 1), (4, 1)]);
    let assignment = round(blocks.state()).into_state();
    assert!(matches!(assignment.pending_decision().unwrap().frame(),
        RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) if frame.chooser() == A));
    // The active player leaves before committing an assignment.
    let timed_out = assignment.expire_response(assignment.basis(), A).unwrap();
    assert!(timed_out.state().pending_decision().is_none());
    assert_eq!(
        timed_out.state().execution().life_total(C),
        Some(&20.into())
    );
    assert!(timed_out.combat_assignments().is_empty());
    // If the blocker owner leaves instead, the other defender still takes damage.
    let block_owner = assignment
        .advance(input(&assignment, B, GameCommand::Concede))
        .unwrap();
    assert!(block_owner.state().pending_decision().is_none());
    assert_eq!(
        block_owner.state().execution().life_total(C),
        Some(&18.into())
    );
    // A bystander leaving keeps the original assignment domain for the attacker.
    let bystander = assignment
        .advance(input(&assignment, D, GameCommand::Concede))
        .unwrap();
    let RuleFrame::Combat(CombatDecision::DamageAssignments(next)) =
        bystander.state().pending_decision().unwrap().frame()
    else {
        panic!("expected damage assignment");
    };
    let RuleFrame::Combat(CombatDecision::DamageAssignments(before)) =
        assignment.pending_decision().unwrap().frame()
    else {
        unreachable!()
    };
    assert_eq!(next.domains(), before.domains());
    assert_eq!(next.chooser(), A);
    assert!(bystander.combat_assignments().is_empty());
}

#[test]
fn defending_player_departure_after_blocks_preserves_other_attacks_and_assigns_no_damage_to_them() {
    for blocks in [vec![], vec![(3, 1)]] {
        let game = attackers(setup(vec![A, B, C, D], cards(), Step::Untap).state());
        let attacking = declared(&game, &[(1, B), (2, C)]);
        let choice = round(attacking.state());
        assert_eq!(choice.state().pending_decision().unwrap().chooser(), B);
        let declarations = blocked(choice.state(), &blocks);
        let before = declarations.state();
        assert!(before.pending_decision().is_none());
        let before_combat = before.execution().combat().unwrap().clone();
        let historical_status = before_combat
            .attacker(object(&game, 1).id())
            .unwrap()
            .block_status();
        let request = input(before, B, GameCommand::Concede);
        let departure = before.advance(request.clone()).unwrap();
        assert_eq!(before.advance(request).unwrap(), departure);
        let next = departure.state();
        assert_eq!(next.execution().window().participants(), &[A, C, D]);
        let combat = next.execution().combat().unwrap();
        assert_eq!(combat.attackers().count(), 2);
        assert_eq!(combat.blockers().count(), 0);
        assert_eq!(
            combat
                .attacker(object(&game, 1).id())
                .unwrap()
                .block_status(),
            historical_status
        );
        assert_eq!(before.execution().combat(), Some(&before_combat));
        let damage = round(next);
        assert_eq!(
            damage.state().turn().unwrap().position().step,
            Step::CombatDamage
        );
        assert_eq!(damage.state().execution().life_total(C), Some(&18.into()));
        assert_eq!(
            damage.state().execution().life_total(B),
            next.execution().life_total(B)
        );
        assert_eq!(damage.state().execution().life_total(D), Some(&20.into()));
    }
}

#[test]
fn multiplayer_actor_order_uses_canonical_apnap_and_invalid_later_reply_keeps_prior_blocks() {
    let mut seeds = cards();
    seeds.extend([battlefield(5, C), battlefield(6, D)]);
    let game = attackers(setup(vec![B, C, A, D], seeds, Step::Untap).state());
    let attacking = declared(&game, &[(2, B), (1, D)]);
    let first = round(attacking.state()).into_state();
    assert_eq!(first.pending_decision().unwrap().chooser(), D);
    let earlier = blocked(&first, &[(6, 1)]);
    assert!(
        matches!(earlier.combat_declarations(), [CombatDeclarationRecord::Blockers { player: D, origin: DeclarationOrigin::PlayerReply, blockers }] if blockers.len() == 1)
    );
    let next = earlier.state();
    assert_eq!(next.pending_decision().unwrap().chooser(), B);
    assert_eq!(next.execution().window().phase(), WindowPhase::TurnWork);
    assert_eq!(next.execution().combat().unwrap().blockers().count(), 1);
    let pending = next.pending_decision().unwrap();
    for choice in [
        vec![BlockDeclaration {
            blocker: object(next, 3).id(),
            attacker: object(next, 1).id(),
        }],
        vec![BlockDeclaration {
            blocker: object(next, 6).id(),
            attacker: object(next, 2).id(),
        }],
        vec![
            BlockDeclaration {
                blocker: object(next, 3).id(),
                attacker: object(next, 2).id()
            };
            2
        ],
    ] {
        let saved = next.clone();
        assert!(
            next.advance(input(
                next,
                B,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::DeclareBlockers(choice)
                }
            ))
            .is_err()
        );
        assert_eq!(next, &saved);
    }
    let completed = blocked(next, &[(3, 2)]);
    assert!(matches!(
        completed.combat_declarations(),
        [CombatDeclarationRecord::Blockers { player: B, .. }]
    ));
    assert!(completed.state().pending_decision().is_none());
    assert_eq!(
        completed
            .state()
            .execution()
            .combat()
            .unwrap()
            .blockers()
            .count(),
        2
    );
    assert_eq!(holder(completed.state()), A);
    assert_eq!(
        completed.state().turn().unwrap().stage(),
        TurnStage::Priority
    );
}

#[test]
fn attacked_actors_without_blockers_have_forced_empty_records_but_nonactors_are_omitted() {
    let seeds: Vec<_> = cards().into_iter().filter(|seed| seed.owner != B).collect();
    let game = attackers(setup(vec![A, D, C, B], seeds, Step::Untap).state());
    let attacking = declared(&game, &[(1, B)]);
    let automatic = round(attacking.state());
    assert!(automatic.state().pending_decision().is_none());
    assert!(
        matches!(automatic.combat_declarations(), [CombatDeclarationRecord::Blockers { player: B, origin: DeclarationOrigin::ForcedEmpty, blockers }] if blockers.is_empty())
    );
    assert_eq!(
        automatic
            .state()
            .execution()
            .combat()
            .unwrap()
            .attacker(object(&game, 1).id())
            .unwrap()
            .block_status(),
        BlockStatus::Unblocked
    );
    assert_eq!(holder(automatic.state()), A);
}

#[test]
fn removing_all_attackers_preserves_history_and_real_blocker_priority_before_damage_stop() {
    let game = attackers(setup(vec![A, B], cards(), Step::Untap).state());
    let attacking = declared(&game, &[(1, B)]).into_state();
    let attacker = object(&attacking, 1).id();
    let cast = act(
        &attacking,
        A,
        GameCommand::Cast {
            spell: object(&attacking, 30).id(),
            target: TargetRef::Object(attacker),
        },
    )
    .into_state();
    let resolved = round(&cast).into_state();
    assert_eq!(
        resolved.turn().unwrap().position().step,
        Step::DeclareAttackers
    );
    assert!(resolved.execution().combat().unwrap().had_attackers());
    assert_eq!(
        resolved.execution().combat().unwrap().attackers().count(),
        0
    );
    let blockers = round(&resolved);
    assert_eq!(
        blockers.state().turn().unwrap().position().step,
        Step::DeclareBlockers
    );
    assert_eq!(holder(blockers.state()), A);
    assert!(blockers.combat_declarations().is_empty());
    assert!(blockers.state().pending_decision().is_none());
    let damage = round(blockers.state());
    assert_eq!(damage.state().pending_turn_work(), None);
    assert_eq!(
        damage.state().turn().unwrap().position().step,
        Step::CombatDamage
    );
    assert_eq!(
        damage.state().turn().unwrap().stage(),
        spellwake_engine::turn::TurnStage::Priority
    );
    assert!(damage.damage_journal().batches.is_empty());
    assert!(damage.combat_assignments().is_empty());
    assert!(damage.state().execution().combat().unwrap().had_attackers());
    let end = round(damage.state());
    assert_eq!(end.state().turn().unwrap().position().step, Step::EndCombat);
    assert!(end.state().execution().combat().is_some());
    let exited = round(end.state());
    assert_eq!(
        exited.state().turn().unwrap().position().step,
        Step::PostcombatMain
    );
    assert!(exited.state().execution().combat().is_none());
}

#[test]
fn removing_last_blocker_preserves_blocked_status_and_removing_attacker_keeps_blocker_membership() {
    for (remove_attacker, spell) in [(false, 30), (true, 30), (false, 32), (true, 32)] {
        let game = attackers(setup(vec![A, B], cards(), Step::Untap).state());
        let attacking = declared(&game, &[(1, B)]);
        let blocking = round(attacking.state());
        let blocked = blocked(blocking.state(), &[(3, 1)]).into_state();
        let attack_id = object(&blocked, 1).id();
        let block_id = object(&blocked, 3).id();
        let departed = if remove_attacker { attack_id } else { block_id };
        let cast = act(
            &blocked,
            A,
            GameCommand::Cast {
                spell: object(&blocked, spell).id(),
                target: TargetRef::Object(departed),
            },
        )
        .into_state();
        let resolved = round(&cast).into_state();
        assert!(
            resolved
                .execution()
                .window()
                .cards()
                .get(departed)
                .is_none()
        );
        let moved = object(&resolved, if remove_attacker { 1 } else { 3 });
        assert_eq!(
            moved.zone(),
            if spell == 30 {
                ObjectZone::Hand(moved.owner())
            } else {
                ObjectZone::Graveyard(moved.owner())
            }
        );
        let combat = resolved.execution().combat().unwrap();
        if remove_attacker {
            assert!(combat.attacker(attack_id).is_none());
            assert_eq!(combat.blocker(block_id).unwrap().attacker(), None);
        } else {
            assert!(combat.blocker(block_id).is_none());
            assert_eq!(
                combat.attacker(attack_id).unwrap().block_status(),
                BlockStatus::Blocked
            );
        }
        assert_eq!(
            resolved.turn().unwrap().position().step,
            Step::DeclareBlockers
        );
    }
}

#[test]
fn prepared_blocker_damage_and_end_boundaries_never_fabricate_combat_context() {
    for (step, work) in [
        (Step::DeclareBlockers, TurnWork::DeclareBlockers),
        (
            Step::CombatDamage,
            TurnWork::CombatDamage { additional: false },
        ),
    ] {
        let initial = setup(vec![A, B], cards(), step);
        let game = initial.state();
        assert_eq!(game.pending_turn_work(), Some(work));
        assert!(game.pending_decision().is_none());
        assert!(game.execution().combat().is_none());
        assert!(initial.combat_declarations().is_empty());
        reject(
            game,
            input(game, A, GameCommand::Pass),
            GameError::TurnWorkRequired(work),
        );
    }
    let end = setup(vec![A, B], cards(), Step::EndCombat).into_state();
    assert!(end.execution().combat().is_none());
    assert!(round(&end).state().execution().combat().is_none());
}

#[test]
fn opening_and_entropy_candidates_keep_all_facts_current_and_preserve_bound_choices() {
    let mut seeds = vec![];
    for player in [A, B] {
        for index in 0..8 {
            seeds.push(creature(
                u64::from(player.0) * 100 + index,
                player,
                ObjectDestination::LibraryBottom,
            ));
        }
    }
    let initial = GameState::initialize_opening(
        GameId(509),
        context(),
        OpeningSetup {
            seed: {
                let cards: Vec<SeedCard> = seeds;
                // Fixture premise: seeded BF timestamps follow this authored card sequence.
                let initial_battlefield_order = cards
                    .iter()
                    .filter(|card| {
                        matches!(
                            card.destination,
                            spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                        )
                    })
                    .map(|card| card.card)
                    .collect();
                GameSeed::with_uniform_life(
                    vec![A, B],
                    A,
                    cards,
                    initial_battlefield_order,
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    test_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            entropy: EntropyTape::new(vec![0; 64]),
        },
    )
    .unwrap();
    assert!(!initial.shuffle_journal().is_empty());
    assert!(!initial.draw_journal().events.is_empty());
    assert!(initial.combat_declarations().is_empty());
    let supplied = initial
        .state()
        .supply_entropy(initial.state().basis(), vec![1, 2])
        .unwrap();
    assert!(supplied.shuffle_journal().is_empty());
    assert!(supplied.draw_journal().events.is_empty());
    assert!(supplied.turn_transitions().is_empty());
    assert!(supplied.combat_declarations().is_empty());
    assert!(supplied.damage_journal().attempts.is_empty());
    assert!(supplied.permanent_entry().is_none());
    assert!(supplied.land_play().is_none());
    assert!(supplied.mana_payment().is_none());
    assert!(supplied.ability_activation().is_none());
    assert!(supplied.mana_production().is_none());
    assert_eq!(
        supplied.state().pending_decision().unwrap().basis(),
        supplied.state().basis()
    );
    let accepted = reply(
        supplied.state(),
        DecisionReply::Mulligan(MulliganChoice::Keep),
    );
    assert!(accepted.shuffle_journal().is_empty());
    assert!(accepted.draw_journal().events.is_empty());
    assert!(accepted.combat_declarations().is_empty());
}

#[test]
fn explicit_empty_blocks_keep_player_origin_and_open_normal_blocker_priority() {
    let game = attackers(setup(vec![A, B], cards(), Step::Untap).state());
    let attacking = declared(&game, &[(1, B), (2, B)]);
    let blocking = round(attacking.state());
    let before = blocking.state().execution().window().cards().clone();
    let accepted = blocked(blocking.state(), &[]);
    assert!(
        matches!(accepted.combat_declarations(), [CombatDeclarationRecord::Blockers { player: B, origin: DeclarationOrigin::PlayerReply, blockers }] if blockers.is_empty())
    );
    assert!(
        accepted
            .state()
            .execution()
            .combat()
            .unwrap()
            .attackers()
            .all(|a| a.block_status() == BlockStatus::Unblocked)
    );
    assert_eq!(accepted.state().execution().window().cards(), &before);
    assert_eq!(holder(accepted.state()), A);
    assert!(round(accepted.state()).combat_declarations().is_empty());
}

#[test]
fn returned_creature_incarnations_cannot_reuse_the_old_attack_handle_or_control_age() {
    let initial = setup(vec![A, B], cards(), Step::Untap).into_state();
    let main = round(&initial).into_state();
    let old = object(&main, 1).id();
    let cast = act(
        &main,
        A,
        GameCommand::Cast {
            spell: object(&main, 30).id(),
            target: TargetRef::Object(old),
        },
    )
    .into_state();
    let returned = round(&cast).into_state();
    let recast = act(
        &returned,
        A,
        GameCommand::CastUntargeted {
            spell: object(&returned, 1).id(),
            payment: None,
        },
    )
    .into_state();
    let resolved = round(&recast).into_state();
    assert_ne!(object(&resolved, 1).id(), old);
    let game = attackers(&resolved);
    let pending = game.pending_decision().unwrap();
    let RuleFrame::Combat(CombatDecision::Attackers(frame)) = pending.frame() else {
        panic!("expected attackers")
    };
    assert_eq!(frame.eligible_attackers(), &[object(&game, 2).id()]);
    let saved = game.clone();
    assert!(
        game.advance(input(
            &game,
            A,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                    attacker: old,
                    defender: B
                }])
            }
        ))
        .is_err()
    );
    assert_eq!(game, saved);
    assert_eq!(
        object(&game, 1).has_continuous_control_since_turn_start(),
        Some(false)
    );
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}
