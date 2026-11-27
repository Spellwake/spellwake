//! Independent acceptance for resumable noncreature tap-mana color choices.
//! Expectations were frozen before implementation in mana-color-choice/reviews/
//! native-scenarios.md. These authored programs qualify no whole cards or games.
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    damage::DamageJournal,
    effects::{
        CopyResolution, CopyTargetChoice, EffectDecision, EffectError, EffectWindow, Instruction,
        ManaColorChoice, Program, SeedCard,
    },
    game::{
        CandidateBase, DecisionId, DecisionReply, GameCommand, GameContext, GameError, GameId,
        GameInput, GameState, NativeCandidate, RuleFrame,
    },
    mana::{FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaType},
    object_identity::{
        CardId, GameObjects, ObjectDestination, ObjectId, ObjectIdentityError, ObjectZone,
    },
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackWindowError, WindowPhase},
    targets::TargetRef,
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const COLORS: [ManaType; 5] = [
    ManaType::White,
    ManaType::Blue,
    ManaType::Black,
    ManaType::Red,
    ManaType::Green,
];

fn context_fields(fields: [&str; 5]) -> GameContext {
    GameContext::new(
        fields[0].into(),
        fields[1].into(),
        fields[2].into(),
        fields[3].into(),
        fields[4].into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn context() -> GameContext {
    context_fields(["release", "rules", "catalogue", "programs", "mana-colors"])
}
fn exact(decimal: &str) -> RulesInteger {
    decimal.to_owned().try_into().unwrap()
}
fn colors(domain: &[ManaType]) -> BTreeSet<ManaType> {
    domain.iter().copied().collect()
}
fn choice(id: u64, amount: RulesInteger, domain: &[ManaType]) -> ManaAbility {
    ManaAbility {
        id: ManaAbilityId(id),
        output: ManaOutput::ChooseColor {
            amount,
            colors: colors(domain),
        },
    }
}
fn fixed(id: u64, mana_type: ManaType, amount: i64) -> ManaAbility {
    ManaAbility {
        id: ManaAbilityId(id),
        output: ManaOutput::Fixed(BTreeMap::from([(mana_type, amount.into())])),
    }
}
fn land(card: u64, owner: SeatId, controller: SeatId, abilities: Vec<ManaAbility>) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Battlefield { controller },
        program: Program::land(std::collections::BTreeSet::new(), abilities),
    }
}
fn instant(
    card: u64,
    owner: SeatId,
    cost: ManaCost,
    instructions: Vec<Instruction>,
    body_target: BodyTarget,
) -> SeedCard {
    SeedCard {
        card: CardId(card),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(cost, ExecutableBody::new(body_target, instructions)),
    }
}
fn creature() -> SeedCard {
    SeedCard {
        card: CardId(99),
        owner: B,
        destination: ObjectDestination::Battlefield { controller: B },
        program: Program::creature(
            None,
            2.into(),
            100.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}
fn nonzero_pools() -> Vec<FloatingMana> {
    [A, B]
        .into_iter()
        .flat_map(|player| {
            ManaType::ALL
                .into_iter()
                .enumerate()
                .map(move |(index, mana_type)| FloatingMana {
                    player,
                    mana_type,
                    amount: RulesInteger::from(index as i64 + if player == A { 1 } else { 11 }),
                })
        })
        .collect()
}
fn setup(cards: Vec<SeedCard>, pools: Vec<FloatingMana>) -> GameState {
    let candidate = GameState::initialize(GameId(84), context(), {
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
            pools,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    })
    .unwrap();
    assert!(candidate.mana_production().is_none());
    assert!(candidate.mana_payment().is_none());
    candidate.into_state()
}
fn world() -> GameState {
    setup(
        vec![
            land(1, A, A, vec![choice(7, 1.into(), &COLORS)]),
            land(2, A, A, vec![choice(7, 1.into(), &COLORS)]),
            instant(
                90,
                A,
                ManaCost::free(),
                vec![spellwake_engine::program::Instruction::DealDamage(
                    Box::new(spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                        exile_if_dies: false,
                    }),
                )],
                BodyTarget::SingleCreature,
            ),
            creature(),
        ],
        nonzero_pools(),
    )
}
fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
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
    let mut expected = game.basis().clone();
    expected.revision += 1;
    assert_eq!(candidate.state().basis(), &expected);
    candidate
}
fn reject(game: &GameState, input: GameInput, error: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(input), Err(error));
    assert_eq!(game, &saved);
}
fn activate(game: &GameState, card: u64, ability: u64) -> GameCommand {
    GameCommand::ActivateMana {
        source: object(game, card),
        ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(ability)),
    }
}
fn balances(game: &GameState) -> Vec<RulesInteger> {
    [A, B]
        .into_iter()
        .flat_map(|player| ManaType::ALL.map(|mana_type| game.execution().mana(player, mana_type)))
        .collect()
}
fn token(game: &GameState) -> &ManaColorChoice {
    let pending = game.pending_decision().expect("pending decision");
    let RuleFrame::ManaColor(frame) = pending.frame() else {
        panic!("expected mana color frame")
    };
    assert_eq!(game.execution().pending_mana_color(), Some(frame));
    assert_eq!(pending.basis(), game.basis());
    assert_eq!(pending.chooser(), frame.chooser());
    frame
}
fn begin(game: &GameState, actor: SeatId, card: u64, ability: u64) -> NativeCandidate {
    let before = balances(game);
    let source = object(game, card);
    let candidate = accept(game, actor, activate(game, card, ability));
    assert!(candidate.mana_production().is_none());
    assert!(candidate.mana_payment().is_none());
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    let next = candidate.state();
    let frame = token(next);
    assert_eq!(frame.source(), source);
    assert_eq!(
        frame.ability(),
        spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(ability))
    );
    assert_eq!(frame.chooser(), actor);
    assert_eq!(balances(next), before);
    assert_eq!(
        next.execution().window().entries(),
        game.execution().window().entries()
    );
    assert_eq!(
        next.execution().window().spell_copies(),
        game.execution().window().spell_copies()
    );
    assert_eq!(object(next, card), source);
    let current = next.execution().window().cards().get(source).unwrap();
    assert_eq!(current.zone(), ObjectZone::Battlefield);
    assert_eq!(current.tapped(), Some(true));
    assert_eq!(
        current.owner(),
        game.execution()
            .window()
            .cards()
            .get(source)
            .unwrap()
            .owner()
    );
    assert_eq!(current.controller(), Some(actor));
    assert!(matches!(
        next.execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
    ));
    assert_eq!(next.execution().instruction_index(), None);
    candidate
}
fn reply_command(game: &GameState, reply: DecisionReply) -> GameCommand {
    GameCommand::Reply {
        decision: game.pending_decision().unwrap().id(),
        choice: reply,
    }
}
fn finish(game: &GameState, selected: ManaType) -> NativeCandidate {
    let frame = token(game).clone();
    let mut expected = balances(game);
    let seat_offset = if frame.chooser() == A { 0 } else { 6 };
    let type_offset = ManaType::ALL
        .iter()
        .position(|value| *value == selected)
        .unwrap();
    expected[seat_offset + type_offset] = expected[seat_offset + type_offset].plus(frame.amount());
    let candidate = accept(
        game,
        frame.chooser(),
        reply_command(game, DecisionReply::ManaColor(selected)),
    );
    let record = candidate
        .mana_production()
        .expect("final reply produces current receipt");
    assert_eq!(record.source, frame.source());
    assert_eq!(record.ability, frame.ability());
    assert_eq!(record.controller, frame.chooser());
    assert_eq!(
        record.produced,
        BTreeMap::from([(selected, frame.amount().clone())])
    );
    assert!(candidate.mana_payment().is_none());
    assert_eq!(candidate.damage_journal(), &DamageJournal::default());
    assert!(candidate.state().pending_decision().is_none());
    assert!(candidate.state().execution().pending_mana_color().is_none());
    assert_eq!(balances(candidate.state()), expected);
    assert_eq!(
        candidate
            .state()
            .execution()
            .window()
            .cards()
            .get(frame.source())
            .unwrap()
            .tapped(),
        Some(true)
    );
    assert_eq!(holder(candidate.state()), frame.chooser());
    candidate
}
fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected ready priority")
    };
    holder
}
fn pass(game: &GameState) -> GameState {
    let candidate = accept(game, holder(game), GameCommand::Pass);
    assert!(candidate.mana_production().is_none());
    assert!(candidate.mana_payment().is_none());
    assert_eq!(balances(candidate.state()), balances(game));
    candidate.into_state()
}
fn cast_free(game: &GameState, actor: SeatId, card: u64, target: TargetRef) -> NativeCandidate {
    accept(
        game,
        actor,
        GameCommand::Cast {
            spell: object(game, card),
            target,
        },
    )
}

#[test]
fn every_color_waits_for_exact_reply_then_credits_only_selected_type() {
    for selected in COLORS {
        let game = world();
        let pending = begin(&game, A, 1, 7);
        assert_eq!(token(pending.state()).amount(), &1.into());
        assert_eq!(token(pending.state()).colors(), &colors(&COLORS));
        finish(pending.state(), selected);
    }
}

#[test]
fn finite_two_color_domain_produces_full_amount_of_one_color() {
    for selected in [ManaType::Blue, ManaType::Green] {
        let game = setup(
            vec![land(
                1,
                A,
                A,
                vec![choice(7, 3.into(), &[ManaType::Blue, ManaType::Green])],
            )],
            nonzero_pools(),
        );
        let pending = begin(&game, A, 1, 7);
        assert_eq!(token(pending.state()).amount(), &3.into());
        assert_eq!(
            token(pending.state()).colors(),
            &colors(&[ManaType::Blue, ManaType::Green])
        );
        finish(pending.state(), selected);
    }
}

#[test]
fn singleton_domain_still_requires_explicit_reply() {
    let game = setup(
        vec![land(1, A, A, vec![choice(7, 2.into(), &[ManaType::Black])])],
        nonzero_pools(),
    );
    let pending = begin(&game, A, 1, 7);
    assert_eq!(token(pending.state()).colors(), &colors(&[ManaType::Black]));
    finish(pending.state(), ManaType::Black);
}

#[test]
fn huge_choice_amount_frame_credit_and_record_are_exact() {
    let amount = exact("900000000000000000000000000000000000007");
    let game = setup(
        vec![land(
            1,
            A,
            A,
            vec![choice(7, amount.clone(), &[ManaType::Red])],
        )],
        vec![
            FloatingMana {
                player: A,
                mana_type: ManaType::Red,
                amount: exact("100000000000000000000000000000000000003"),
            },
            FloatingMana {
                player: B,
                mana_type: ManaType::Red,
                amount: 41.into(),
            },
        ],
    );
    let pending = begin(&game, A, 1, 7);
    assert_eq!(token(pending.state()).amount(), &amount);
    let candidate = finish(pending.state(), ManaType::Red);
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Red),
        exact("1000000000000000000000000000000000000010")
    );
}

#[test]
fn explicit_ability_id_selects_actual_domain_and_amount() {
    let game = setup(
        vec![land(
            1,
            A,
            A,
            vec![
                choice(2, 1.into(), &[ManaType::White]),
                choice(9, 4.into(), &[ManaType::Blue, ManaType::Black]),
            ],
        )],
        nonzero_pools(),
    );
    let pending = begin(&game, A, 1, 9);
    assert_eq!(token(pending.state()).amount(), &4.into());
    assert_eq!(
        token(pending.state()).colors(),
        &colors(&[ManaType::Blue, ManaType::Black])
    );
    finish(pending.state(), ManaType::Black);
}

#[test]
fn fixed_and_choice_outputs_share_source_without_changing_fixed_colorless() {
    let game = setup(
        vec![land(
            1,
            A,
            A,
            vec![
                fixed(1, ManaType::Colourless, 2),
                choice(7, 3.into(), &COLORS),
            ],
        )],
        nonzero_pools(),
    );
    let candidate = accept(&game, A, activate(&game, 1, 1));
    assert!(candidate.state().pending_decision().is_none());
    assert_eq!(
        candidate.mana_production().unwrap().produced,
        BTreeMap::from([(ManaType::Colourless, 2.into())])
    );
    assert_eq!(
        candidate.state().execution().mana(A, ManaType::Colourless),
        8.into()
    );
    assert!(candidate.mana_payment().is_none());
    let pending = begin(&game, A, 1, 7);
    finish(pending.state(), ManaType::Green);
}

#[test]
fn controller_chooses_and_receives_mana_while_foreign_owner_remains_owner() {
    let game = setup(
        vec![land(1, B, A, vec![choice(7, 3.into(), &COLORS)])],
        nonzero_pools(),
    );
    let pending = begin(&game, A, 1, 7);
    assert_eq!(
        pending
            .state()
            .execution()
            .window()
            .cards()
            .get(object(&game, 1))
            .unwrap()
            .owner(),
        B
    );
    reject(
        pending.state(),
        request(
            pending.state(),
            B,
            reply_command(pending.state(), DecisionReply::ManaColor(ManaType::Green)),
        ),
        GameError::WrongChooser,
    );
    finish(pending.state(), ManaType::Green);
}

#[test]
fn pending_tap_preserves_a_real_nonempty_stack_and_allocates_no_ability_entry() {
    let game = world();
    let cast = cast_free(&game, A, 90, TargetRef::Object(object(&game, 99)));
    assert_eq!(cast.state().execution().window().entries().len(), 1);
    let source = cast.mana_payment().unwrap().source;
    let pending = begin(cast.state(), A, 1, 7);
    let final_reply = finish(pending.state(), ManaType::White);
    assert_eq!(final_reply.state().execution().window().entries().len(), 1);
    assert_eq!(
        final_reply.state().execution().window().entries()[0].id(),
        source
    );
}

#[test]
fn final_choice_retains_actor_priority_and_resets_an_earlier_pass() {
    let game = setup(
        vec![land(1, B, B, vec![choice(7, 1.into(), &COLORS)])],
        nonzero_pools(),
    );
    let game = pass(&game);
    assert_eq!(holder(&game), B);
    let pending = begin(&game, B, 1, 7);
    let completed = finish(pending.state(), ManaType::Blue).into_state();
    let after_b_pass = pass(&completed);
    assert_eq!(holder(&after_b_pass), A);
    let after_a_pass = pass(&after_b_pass);
    assert!(matches!(
        after_a_pass.execution().window().phase(),
        WindowPhase::StepEnded
    ));
}

#[test]
fn later_pass_and_unrelated_cast_never_repeat_production() {
    let game = world();
    let pending = begin(&game, A, 1, 7);
    let completed = finish(pending.state(), ManaType::Red).into_state();
    let cast = cast_free(&completed, A, 90, TargetRef::Object(object(&completed, 99)));
    assert!(cast.mana_production().is_none());
    assert!(cast.mana_payment().is_some());
    assert_eq!(balances(cast.state()), balances(&completed));
    let game = pass(cast.state());
    let game = pass(&game);
    assert!(game.pending_decision().is_none());
}

fn invalid_definition(abilities: Vec<ManaAbility>) {
    assert_eq!(
        EffectWindow::new(GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![land(1, A, A, abilities)],
            vec![CardId(1)],
            nonzero_pools(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )),
        Err(EffectError::InvalidSetup)
    );
}

#[test]
fn setup_rejects_empty_nonpositive_and_colorless_choice_domains() {
    invalid_definition(vec![choice(7, 1.into(), &[])]);
    invalid_definition(vec![choice(7, 1.into(), &[ManaType::Colourless])]);
    invalid_definition(vec![choice(
        7,
        1.into(),
        &[ManaType::White, ManaType::Colourless],
    )]);
    invalid_definition(vec![choice(7, 0.into(), &COLORS)]);
    invalid_definition(vec![choice(7, (-1).into(), &COLORS)]);
}

#[test]
fn setup_preserves_fixed_validation_and_source_local_unique_ids() {
    let empty_land = setup(vec![land(1, A, A, vec![])], nonzero_pools());
    reject(
        &empty_land,
        request(&empty_land, A, activate(&empty_land, 1, 7)),
        GameError::Effect(EffectError::NotManaSource),
    );
    invalid_definition(vec![ManaAbility {
        id: ManaAbilityId(7),
        output: ManaOutput::Fixed(BTreeMap::new()),
    }]);
    invalid_definition(vec![fixed(7, ManaType::Red, 0)]);
    invalid_definition(vec![fixed(7, ManaType::Red, -1)]);
    invalid_definition(vec![
        fixed(7, ManaType::Red, 1),
        choice(7, 1.into(), &COLORS),
    ]);
    let game = world();
    assert_eq!(
        token(begin(&game, A, 1, 7).state()).ability(),
        spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7))
    );
    assert_eq!(
        token(begin(&game, A, 2, 7).state()).ability(),
        spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7))
    );
}

#[test]
fn choice_activation_preserves_source_ability_controller_and_priority_legality() {
    let game = world();
    reject(
        &game,
        request(&game, A, activate(&game, 1, 99)),
        GameError::Effect(EffectError::UnknownManaAbility),
    );
    for card in [90, 99] {
        reject(
            &game,
            request(&game, A, activate(&game, card, 7)),
            GameError::Effect(EffectError::NotManaSource),
        );
    }
    reject(
        &game,
        request(&game, B, activate(&game, 1, 7)),
        GameError::Effect(EffectError::Window(StackWindowError::Priority(
            PriorityError::WrongPlayer,
        ))),
    );
    let foreign = setup(
        vec![land(1, A, B, vec![choice(7, 1.into(), &COLORS)])],
        nonzero_pools(),
    );
    reject(
        &foreign,
        request(&foreign, A, activate(&foreign, 1, 7)),
        GameError::Effect(EffectError::Window(StackWindowError::WrongController)),
    );
    let pending = begin(&game, A, 1, 7).into_state();
    let completed = finish(&pending, ManaType::Blue).into_state();
    reject(
        &completed,
        request(&completed, A, activate(&completed, 1, 7)),
        GameError::Effect(EffectError::Window(StackWindowError::SourceTapped)),
    );
}

#[test]
fn nonbattlefield_choice_sources_have_no_payable_tap_status() {
    for destination in [
        ObjectDestination::Hand,
        ObjectDestination::LibraryBottom,
        ObjectDestination::Graveyard,
        ObjectDestination::Exile,
        ObjectDestination::Command,
    ] {
        let mut source = land(1, A, A, vec![choice(7, 1.into(), &COLORS)]);
        source.destination = destination;
        let game = setup(vec![source], nonzero_pools());
        reject(
            &game,
            request(&game, A, activate(&game, 1, 7)),
            GameError::Effect(EffectError::Window(StackWindowError::Objects(
                ObjectIdentityError::NotPermanent,
            ))),
        );
    }
}

#[test]
fn unknown_source_handle_cannot_begin_a_choice_even_on_current_game_basis() {
    let game = world();
    let mut cards = GameObjects::new(0);
    let mut absent = object(&game, 1);
    for card in 1..=5 {
        let seeded = cards
            .seed_card(cards.revision(), CardId(card), A, ObjectDestination::Hand)
            .unwrap();
        absent = seeded.after.id();
        cards = seeded.next;
    }
    assert!(game.execution().window().cards().get(absent).is_none());
    reject(
        &game,
        request(
            &game,
            A,
            GameCommand::ActivateMana {
                source: absent,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            },
        ),
        GameError::Effect(EffectError::UnknownObject),
    );
}

#[test]
fn synchronous_choice_convenience_rejects_without_accepting_tap_or_credit() {
    let game = world();
    let state = game.execution();
    let saved = state.clone();
    assert_eq!(
        state.activate_mana(
            state.revision(),
            A,
            object(&game, 1),
            spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7))
        ),
        Err(EffectError::ManaColorChoiceRequired)
    );
    assert_eq!(state, &saved);
    assert_eq!(
        state
            .window()
            .cards()
            .get(object(&game, 1))
            .unwrap()
            .tapped(),
        Some(false)
    );
    assert!(state.pending_mana_color().is_none());
}

#[test]
fn direct_fixed_begin_and_synchronous_convenience_have_identical_successors() {
    let game = setup(
        vec![land(1, A, A, vec![fixed(7, ManaType::Colourless, 2)])],
        nonzero_pools(),
    );
    let state = game.execution();
    let synchronous = state
        .activate_mana(
            state.revision(),
            A,
            object(&game, 1),
            spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        )
        .unwrap();
    let (begun, spellwake_engine::effects::ManaActivationResult::Complete { production }, _) =
        state
            .begin_mana_activation(
                state.revision(),
                A,
                object(&game, 1),
                spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            )
            .unwrap()
            .into_parts()
    else {
        panic!("fixed output must complete")
    };
    assert_eq!(&begun, synchronous.state());
    assert_eq!(&production, synchronous.result());
    assert!(begun.pending_mana_color().is_none());
}

#[test]
fn illegal_color_replies_preserve_accepted_tap_and_allow_a_later_valid_reply() {
    let game = setup(
        vec![land(1, A, A, vec![choice(7, 2.into(), &[ManaType::Blue])])],
        nonzero_pools(),
    );
    let pending = begin(&game, A, 1, 7).into_state();
    for selected in [ManaType::Colourless, ManaType::White] {
        reject(
            &pending,
            request(
                &pending,
                A,
                reply_command(&pending, DecisionReply::ManaColor(selected)),
            ),
            GameError::Effect(EffectError::IllegalManaColor),
        );
        assert_eq!(
            pending
                .execution()
                .window()
                .cards()
                .get(object(&pending, 1))
                .unwrap()
                .tapped(),
            Some(true)
        );
        assert_eq!(balances(&pending), balances(&game));
    }
    finish(&pending, ManaType::Blue);
}

#[test]
fn exact_basis_actor_id_and_reply_kind_are_required_for_game_choice() {
    let game = world();
    let pending = begin(&game, A, 1, 7).into_state();
    let legal = request(
        &pending,
        A,
        reply_command(&pending, DecisionReply::ManaColor(ManaType::White)),
    );
    let mut wrong = legal.clone();
    wrong.actor = B;
    reject(&pending, wrong, GameError::WrongChooser);
    let mut wrong = legal.clone();
    wrong.actor = SeatId(77);
    reject(&pending, wrong, GameError::UnknownActor);
    let mut wrong = legal.clone();
    wrong.basis.game = GameId(999);
    reject(&pending, wrong, GameError::WrongGame);
    let mut wrong = legal.clone();
    wrong.basis.epoch += 1;
    reject(&pending, wrong, GameError::WrongEpoch);
    let mut wrong = legal.clone();
    wrong.basis.revision = game.basis().revision;
    reject(&pending, wrong, GameError::StaleRevision);
    for index in 0..5 {
        let mut fields = ["release", "rules", "catalogue", "programs", "mana-colors"];
        fields[index] = "different";
        let mut wrong = legal.clone();
        wrong.basis.context = context_fields(fields);
        reject(&pending, wrong, GameError::WrongContext);
    }
    reject(
        &pending,
        request(
            &pending,
            A,
            GameCommand::Reply {
                decision: DecisionId(pending.pending_decision().unwrap().id().0 + 1),
                choice: DecisionReply::ManaColor(ManaType::White),
            },
        ),
        GameError::WrongDecision,
    );
    reject(
        &pending,
        request(
            &pending,
            A,
            reply_command(
                &pending,
                DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            ),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    finish(&pending, ManaType::White);
}

#[test]
fn all_ordinary_game_commands_reject_while_choice_is_pending() {
    let game = world();
    let pending = begin(&game, A, 1, 7).into_state();
    let target = TargetRef::Object(object(&pending, 99));
    for command in [
        GameCommand::Pass,
        GameCommand::Cast {
            spell: object(&pending, 90),
            target,
        },
        GameCommand::CastPaid {
            spell: object(&pending, 90),
            target,
            payment: ManaPayment::default(),
        },
        activate(&pending, 2, 7),
    ] {
        for actor in [A, B] {
            reject(
                &pending,
                request(&pending, actor, command.clone()),
                GameError::DecisionRequired,
            );
        }
    }
    finish(&pending, ManaType::Green);
}

#[test]
fn direct_executor_choice_token_is_exact_and_color_validation_is_atomic() {
    let game = world();
    let execution = game.execution();
    let (state, spellwake_engine::effects::ManaActivationResult::Pending { frame }, _) = execution
        .begin_mana_activation(
            execution.revision(),
            A,
            object(&game, 1),
            spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
        )
        .unwrap()
        .into_parts()
    else {
        panic!("expected pending")
    };
    assert_eq!(state.pending_mana_color(), Some(&frame));
    assert_eq!(
        state.mana(A, ManaType::White),
        execution.mana(A, ManaType::White)
    );
    let saved = state.clone();
    assert_eq!(
        state.resume_mana_color(state.revision() - 1, &frame, A, ManaType::White),
        Err(EffectError::Window(StackWindowError::StaleRevision))
    );
    assert_eq!(
        state.resume_mana_color(state.revision(), &frame, B, ManaType::White),
        Err(EffectError::WrongChooser)
    );
    assert_eq!(
        state.resume_mana_color(state.revision(), &frame, A, ManaType::Colourless),
        Err(EffectError::IllegalManaColor)
    );
    assert_eq!(state, saved);
    let completed = state
        .resume_mana_color(state.revision(), &frame, A, ManaType::White)
        .unwrap();
    assert_eq!(
        completed.result().produced,
        BTreeMap::from([(ManaType::White, 1.into())])
    );
    assert!(completed.state().pending_mana_color().is_none());
    assert_eq!(completed.state().mana(A, ManaType::White), 2.into());
    assert_eq!(
        completed
            .state()
            .window()
            .cards()
            .get(frame.source())
            .unwrap()
            .tapped(),
        Some(true)
    );
}

#[test]
fn genuinely_foreign_source_ability_amount_and_domain_frames_never_resume() {
    let game = world();
    let pending = begin(&game, A, 1, 7).into_state();
    let mut foreign = vec![token(begin(&game, A, 2, 7).state()).clone()];
    let intervening_cast = cast_free(&game, A, 90, TargetRef::Object(object(&game, 99)));
    foreign.push(token(begin(intervening_cast.state(), A, 1, 7).state()).clone());
    for ability in [
        choice(8, 1.into(), &COLORS),
        choice(7, 3.into(), &COLORS),
        choice(7, 1.into(), &[ManaType::White, ManaType::Blue]),
    ] {
        let id = ability.id.0;
        let other = setup(vec![land(1, A, A, vec![ability])], nonzero_pools());
        foreign.push(token(begin(&other, A, 1, id).state()).clone());
    }
    let state = pending.execution();
    let saved = state.clone();
    for frame in foreign {
        assert_eq!(
            state.resume_mana_color(state.revision(), &frame, A, ManaType::White),
            Err(EffectError::WrongManaColorFrame)
        );
        assert_eq!(state, &saved);
    }
    finish(&pending, ManaType::White);
}

#[test]
fn frame_and_decision_cannot_be_replayed_after_completion_or_into_later_choice() {
    let game = world();
    let pending = begin(&game, A, 1, 7).into_state();
    let old_frame = token(&pending).clone();
    let old_decision = pending.pending_decision().unwrap().id();
    let completed = finish(&pending, ManaType::Green).into_state();
    let saved = completed.execution().clone();
    assert_eq!(
        completed.execution().resume_mana_color(
            completed.execution().revision(),
            &old_frame,
            A,
            ManaType::Green
        ),
        Err(EffectError::WrongManaColorFrame)
    );
    assert_eq!(completed.execution(), &saved);
    reject(
        &completed,
        request(
            &completed,
            A,
            GameCommand::Reply {
                decision: old_decision,
                choice: DecisionReply::ManaColor(ManaType::Green),
            },
        ),
        GameError::NoDecision,
    );
    let later = begin(&completed, A, 2, 7).into_state();
    assert_ne!(later.pending_decision().unwrap().id(), old_decision);
    assert_eq!(
        later.execution().resume_mana_color(
            later.execution().revision(),
            &old_frame,
            A,
            ManaType::Green
        ),
        Err(EffectError::WrongManaColorFrame)
    );
    reject(
        &later,
        request(
            &later,
            A,
            GameCommand::Reply {
                decision: old_decision,
                choice: DecisionReply::ManaColor(ManaType::Green),
            },
        ),
        GameError::WrongDecision,
    );
    finish(&later, ManaType::Red);
}

#[test]
fn direct_executor_ordinary_actions_and_checks_are_blocked_before_credit() {
    let game = world();
    let pending = begin(&game, A, 1, 7).into_state();
    let state = pending.execution();
    let saved = state.clone();
    let target = TargetRef::Object(object(&pending, 99));
    assert_eq!(
        state.pass(state.revision(), A),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.cast(state.revision(), A, object(&pending, 90), target),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.cast_with_payment(
            state.revision(),
            A,
            object(&pending, 90),
            target,
            Some(ManaPayment::default())
        ),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.activate_mana(
            state.revision(),
            A,
            object(&pending, 2),
            spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7))
        ),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.begin_mana_activation(
            state.revision(),
            A,
            object(&pending, 2),
            spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7))
        ),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.settle_checks(state.revision()),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.begin_resolution(state.revision()),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.resolve_top(state.revision()),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.resolve_top_with_copy_choice(state.revision(), A, CopyTargetChoice::Retain),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(state, &saved);
    finish(&pending, ManaType::Black);
}

fn copy_pending() -> GameState {
    let game = setup(
        vec![
            land(1, A, A, vec![choice(7, 1.into(), &COLORS)]),
            instant(
                90,
                A,
                ManaCost::free(),
                vec![spellwake_engine::program::Instruction::DealDamage(
                    Box::new(spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            spellwake_engine::program::ValueExpr::Literal(1.into()),
                        ),
                        exile_if_dies: false,
                    }),
                )],
                BodyTarget::SingleCreature,
            ),
            instant(
                91,
                B,
                ManaCost::free(),
                vec![Instruction::StackOp(Box::new(
                    spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
                ))],
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
            ),
            creature(),
        ],
        nonzero_pools(),
    );
    let original = cast_free(&game, A, 90, TargetRef::Object(object(&game, 99)));
    let original_id = original.mana_payment().unwrap().source;
    let game = pass(original.state());
    let copy = cast_free(&game, B, 91, TargetRef::Stack(original_id));
    pass(&pass(copy.state()))
}
fn copy_frame(game: &GameState) -> CopyResolution {
    let RuleFrame::CopyTargets(frame) = game.pending_decision().unwrap().frame() else {
        panic!("expected copy")
    };
    frame.clone()
}
fn damage_pending() -> GameState {
    let game = setup(
        vec![
            land(1, A, A, vec![choice(7, 1.into(), &COLORS)]),
            instant(
                90,
                A,
                ManaCost::free(),
                vec![
                    Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: None, payload: spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield { amount: 1.into() }, duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: true, as_emblem: None, emblem_label: None })),
                    Instruction::ApplyContinuous(Box::new(spellwake_engine::toolbox::ApplyContinuousSpec { selector: None, payload: spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield { amount: 1.into() }, duration: spellwake_engine::characteristics::ResolvedDuration::UntilEndOfTurn, condition: None, one_shot: true, as_emblem: None, emblem_label: None })),
                    spellwake_engine::program::Instruction::DealDamage(Box::new(
                        spellwake_engine::toolbox::DealDamageSpec {
                            source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                            recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            amount: spellwake_engine::program::DamageAmountSpec::Expr(
                                spellwake_engine::program::ValueExpr::Literal(3.into()),
                            ),
                            exile_if_dies: false,
                        },
                    )),
                ],
                BodyTarget::SingleCreature,
            ),
            creature(),
        ],
        nonzero_pools(),
    );
    let cast = cast_free(&game, A, 90, TargetRef::Object(object(&game, 99)));
    pass(&pass(cast.state()))
}

#[test]
fn resolution_resume_wrappers_cannot_cross_into_pending_mana_work() {
    let copy = copy_pending();
    let frame = copy_frame(&copy);
    let game = world();
    let pending = begin(&game, A, 1, 7).into_state();
    let state = pending.execution();
    let saved = state.clone();
    assert_eq!(
        state.resume_rule(
            state.revision(),
            &EffectDecision::CopyTargets(frame.clone()),
            B,
            DecisionReply::CopyTargets(CopyTargetChoice::Retain),
            None
        ),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(
        state.resume_copy_resolution(state.revision(), &frame, B, CopyTargetChoice::Retain),
        Err(EffectError::WrongPhase)
    );
    let damage = damage_pending();
    let RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(frame)) =
        damage.pending_decision().unwrap().frame()
    else {
        panic!("expected damage")
    };
    reject(
        &pending,
        request(
            &pending,
            A,
            reply_command(
                &pending,
                DecisionReply::DamageOrder(frame.applications()[0]),
            ),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    assert_eq!(
        state.resume_rule(
            state.revision(),
            &EffectDecision::Damage(spellwake_engine::effects::DamageDecision::Order(
                frame.clone()
            )),
            B,
            DecisionReply::DamageOrder(frame.applications()[0]),
            None
        ),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(state, &saved);
    finish(&pending, ManaType::Blue);
}

#[test]
fn color_reply_cannot_replace_copy_continuation_and_copy_still_completes() {
    let game = copy_pending();
    assert!(matches!(
        game.pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    reject(
        &game,
        request(
            &game,
            B,
            reply_command(&game, DecisionReply::ManaColor(ManaType::Blue)),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    reject(
        &game,
        request(&game, A, activate(&game, 1, 7)),
        GameError::DecisionRequired,
    );
    let state = game.execution();
    assert_eq!(
        state.begin_mana_activation(
            state.revision(),
            A,
            object(&game, 1),
            spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(7))
        ),
        Err(EffectError::Window(StackWindowError::NotInPriority))
    );
    let candidate = accept(
        &game,
        B,
        reply_command(&game, DecisionReply::CopyTargets(CopyTargetChoice::Retain)),
    );
    assert!(candidate.mana_production().is_none());
    assert!(candidate.mana_payment().is_none());
    assert_eq!(balances(candidate.state()), balances(&game));
    assert_eq!(
        candidate.state().execution().window().spell_copies().len(),
        1
    );
}

#[test]
fn color_reply_cannot_replace_damage_continuation_and_damage_still_completes() {
    let game = damage_pending();
    let RuleFrame::Damage(spellwake_engine::effects::DamageDecision::Order(frame)) =
        game.pending_decision().unwrap().frame()
    else {
        panic!("expected damage")
    };
    let selected = frame.applications()[0];
    reject(
        &game,
        request(
            &game,
            B,
            reply_command(&game, DecisionReply::ManaColor(ManaType::Green)),
        ),
        GameError::Effect(EffectError::WrongDecisionKind),
    );
    reject(
        &game,
        request(&game, A, activate(&game, 1, 7)),
        GameError::DecisionRequired,
    );
    let candidate = accept(
        &game,
        B,
        reply_command(&game, DecisionReply::DamageOrder(selected)),
    );
    assert!(candidate.mana_production().is_none());
    assert!(candidate.mana_payment().is_none());
    assert_eq!(balances(candidate.state()), balances(&game));
    assert_eq!(
        candidate
            .state()
            .execution()
            .creature(object(&game, 99))
            .unwrap()
            .unwrap()
            .damage,
        1.into()
    );
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("rules")
}
