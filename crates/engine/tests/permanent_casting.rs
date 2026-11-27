#![allow(dead_code, unused_imports)]
//! Independent public-boundary checks for authored creature/artifact casting.
//! Prepared phases and literal programs do not certify complete cards or games.
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::setup::GameSeed;
use spellwake_engine::{
    actions::TimingError,
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::{CopyTargetChoice, EffectError, Instruction, Program, SeedCard},
    game::{
        CandidateBase, DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput,
        GameState, NativeCandidate, OpeningSetup, RuleFrame,
    },
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment,
        ManaPaymentError, ManaSpend, ManaType,
    },
    object_identity::{CardId, GameObject, ObjectDestination, ObjectId, ObjectZone},
    opening::MulliganChoice,
    priority::{PriorityPhase, SeatId},
    shuffle::EntropyTape,
    stack_window::{StackKind, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart, TurnWork},
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);

fn context() -> GameContext {
    GameContext::new(
        "permanent-test".into(),
        "cr-20260925".into(),
        "literal-fixtures".into(),
        "partial-permanents".into(),
        "ordinary-individual-turn".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn creature(
    id: u64,
    owner: SeatId,
    destination: ObjectDestination,
    casting_cost: Option<ManaCost>,
    toughness: i64,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::creature(
            casting_cost,
            2.into(),
            toughness.into(),
            std::collections::BTreeSet::new(),
        ),
    }
}

fn artifact(id: u64, owner: SeatId, casting_cost: Option<ManaCost>) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::artifact(casting_cost, vec![]),
    }
}

fn tap_ability() -> ActivatedAbility {
    ActivatedAbility {
        id: ActivatedAbilityId(7),
        mana_cost: ManaCost::one(ManaType::Green),
        source_cost: SourceCost::Tap,
        body: ExecutableBody::new(
            BodyTarget::SingleCreature,
            vec![spellwake_engine::program::Instruction::DealDamage(
                Box::new(spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(2.into()),
                    ),
                    exile_if_dies: false,
                }),
            )],
        ),
    }
}

fn spell(
    id: u64,
    owner: SeatId,
    instruction: Instruction,
    cost: ManaCost,
    body_target: BodyTarget,
) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(cost, ExecutableBody::new(body_target, vec![instruction])),
    }
}

fn land(id: u64, destination: ObjectDestination, choice: bool) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner: A,
        destination,
        program: Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: if choice {
                    ManaOutput::ChooseColor {
                        amount: 1.into(),
                        colors: BTreeSet::from([ManaType::Blue, ManaType::Green]),
                    }
                } else {
                    ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())]))
                },
            }],
        ),
    }
}

fn cost() -> ManaCost {
    ManaCost {
        generic: 1.into(),
        required: BTreeMap::from([(ManaType::White, 1.into())]),
    }
}

fn payment(entries: &[(ManaType, i64)]) -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: entries
            .iter()
            .map(|&(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    }
}

fn ordinary_payment() -> ManaPayment {
    payment(&[(ManaType::White, 1), (ManaType::Red, 1)])
}

fn pools() -> Vec<FloatingMana> {
    [A, B]
        .into_iter()
        .flat_map(|player| {
            ManaType::ALL.map(move |mana_type| FloatingMana {
                player,
                mana_type,
                amount: if player == A { 20.into() } else { 30.into() },
            })
        })
        .collect()
}

fn initialize(
    step: Option<Step>,
    cards: Vec<SeedCard>,
    mana: Vec<FloatingMana>,
) -> NativeCandidate {
    match step {
        Some(step) => GameState::initialize_scheduled(
            GameId(6083),
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
                    vec![A, B],
                    A,
                    cards,
                    initial_battlefield_order,
                    mana,
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    test_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            TurnStart { number: 3, step },
        ),
        None => GameState::initialize(GameId(6083), context(), {
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
                mana,
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        }),
    }
    .unwrap()
}

fn setup(step: Option<Step>, cards: Vec<SeedCard>) -> GameState {
    let initial = initialize(step, cards, pools());
    no_action_records(&initial);
    initial.into_state()
}

fn object(game: &GameState, card: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn physical(game: &GameState, card: u64) -> &GameObject {
    game.execution()
        .window()
        .cards()
        .get(object(game, card))
        .unwrap()
}

fn holder(game: &GameState) -> SeatId {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!(
            "expected ready fixture, got {:?}",
            game.execution().window().phase()
        );
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

fn accept(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    let saved = game.clone();
    let candidate = game.advance(input(game, actor, command)).unwrap();
    assert_eq!(
        game, &saved,
        "building a candidate mutated the accepted state"
    );
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(game.basis().clone())
    );
    let mut basis = game.basis().clone();
    basis.revision += 1;
    assert_eq!(candidate.state().basis(), &basis);
    candidate
}

fn reject_input(game: &GameState, request: GameInput, error: GameError) {
    let saved = game.clone();
    assert_eq!(game.advance(request), Err(error));
    assert_eq!(game, &saved, "rejection changed the accepted state");
}

fn reject(game: &GameState, actor: SeatId, command: GameCommand, error: EffectError) {
    reject_input(game, input(game, actor, command), GameError::Effect(error));
}

fn untargeted(game: &GameState, card: u64, selected: Option<ManaPayment>) -> GameCommand {
    GameCommand::CastUntargeted {
        spell: object(game, card),
        payment: selected,
    }
}

fn cast(
    game: &GameState,
    actor: SeatId,
    card: u64,
    selected: Option<ManaPayment>,
) -> NativeCandidate {
    let candidate = accept(game, actor, untargeted(game, card, selected));
    assert!(candidate.permanent_entry().is_none());
    assert!(candidate.mana_payment().is_some());
    assert!(candidate.ability_activation().is_none());
    assert!(candidate.mana_production().is_none());
    assert!(candidate.land_play().is_none());
    assert!(candidate.damage_journal().applied.is_empty());
    candidate
}

fn targeted(game: &GameState, actor: SeatId, card: u64, target: TargetRef) -> NativeCandidate {
    accept(
        game,
        actor,
        GameCommand::Cast {
            spell: object(game, card),
            target,
        },
    )
}

fn pass(game: &GameState) -> NativeCandidate {
    accept(game, holder(game), GameCommand::Pass)
}

fn resolve(game: &GameState) -> NativeCandidate {
    let first = pass(game);
    no_action_records(&first);
    pass(first.state())
}

fn balances(game: &GameState) -> Vec<RulesInteger> {
    [A, B]
        .into_iter()
        .flat_map(|player| ManaType::ALL.map(|mana_type| game.execution().mana(player, mana_type)))
        .collect()
}

fn no_action_records(candidate: &NativeCandidate) {
    assert!(candidate.permanent_entry().is_none());
    assert!(candidate.mana_payment().is_none());
    assert!(candidate.ability_activation().is_none());
    assert!(candidate.mana_production().is_none());
    assert!(candidate.land_play().is_none());
}

#[test]
fn invalid_executable_costs_reject_at_setup_without_relaxing_artifact_ability_validation() {
    for invalid in [
        ManaCost::generic((-1).into()),
        ManaCost::specific(ManaType::Blue, (-1).into()),
    ] {
        for seed in [
            creature(1, A, ObjectDestination::Hand, Some(invalid.clone()), 3),
            artifact(1, A, Some(invalid)),
        ] {
            assert_eq!(
                GameState::initialize(GameId(6083), context(), {
                    let cards: Vec<SeedCard> = vec![seed];
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
                        pools(),
                        20.into(),
                        CombatRules::UnrestrictedFreeForAll,
                        test_vocabulary(),
                        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                    )
                }),
                Err(GameError::Effect(EffectError::InvalidSetup))
            );
        }
    }
    let mut duplicate = tap_ability();
    let duplicate_ids = vec![duplicate.clone(), duplicate.clone()];
    duplicate.mana_cost = ManaCost::generic((-1).into());
    let negative = vec![duplicate];
    let mut empty = tap_ability();
    empty.body.instructions.clear();
    for abilities in [duplicate_ids, negative, vec![empty]] {
        let seed = SeedCard {
            card: CardId(1),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::artifact(Some(ManaCost::free()), abilities),
        };
        assert_eq!(
            GameState::initialize(GameId(6083), context(), {
                let cards: Vec<SeedCard> = vec![seed];
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
                    pools(),
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    test_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            }),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
    }
}

#[test]
fn required_only_payment_is_unique_and_arbitrary_precision_costs_remain_exact() {
    let exact: RulesInteger = "90071992547409931234567890".to_owned().try_into().unwrap();
    let cards = vec![creature(
        1,
        A,
        ObjectDestination::Hand,
        Some(ManaCost::specific(ManaType::White, exact.clone())),
        3,
    )];
    let game = initialize(
        Some(Step::PrecombatMain),
        cards,
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::White,
            amount: exact.plus(&3.into()),
        }],
    )
    .into_state();
    let cast = cast(&game, A, 1, None);
    assert_eq!(cast.mana_payment().unwrap().cost.total(), exact);
    assert_eq!(cast.state().execution().mana(A, ManaType::White), 3.into());
    let done = resolve(cast.state());
    assert_eq!(done.state().execution().mana(A, ManaType::White), 3.into());
    assert!(done.permanent_entry().is_some());
}

#[test]
fn opening_choices_and_host_entropy_candidates_never_emit_permanent_entry() {
    let cards: Vec<SeedCard> = [A, B]
        .into_iter()
        .enumerate()
        .flat_map(|(index, owner)| {
            (0..8).map(move |ordinal| {
                creature(
                    1000 + index as u64 * 100 + ordinal,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Some(ManaCost::free()),
                    3,
                )
            })
        })
        .collect();
    let initial = GameState::initialize_opening(
        GameId(6083),
        context(),
        OpeningSetup {
            seed: {
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
            entropy: EntropyTape::new(vec![0; 100]),
        },
    )
    .unwrap();
    no_action_records(&initial);
    let game = initial.state();
    let hand = game
        .execution()
        .window()
        .cards()
        .iter()
        .find(|card| card.zone() == ObjectZone::Hand(A))
        .unwrap()
        .id();
    reject_input(
        game,
        input(
            game,
            A,
            GameCommand::CastUntargeted {
                spell: hand,
                payment: None,
            },
        ),
        GameError::DecisionRequired,
    );
    let refill = game.supply_entropy(game.basis(), vec![0, 1]).unwrap();
    no_action_records(&refill);
    let decision = refill.state().pending_decision().unwrap();
    let keep = accept(
        refill.state(),
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::Mulligan(MulliganChoice::Keep),
        },
    );
    no_action_records(&keep);
    assert!(keep.state().pending_decision().is_some());
}

#[test]
fn final_pass_only_resolves_targetless_permanent_for_two_through_six_seats() {
    for count in 2..=6 {
        let seats: Vec<_> = (0..count).map(SeatId).collect();
        let active = seats[0];
        let game = GameState::initialize_scheduled(
            GameId(6083),
            context(),
            GameSeed::with_uniform_life(
                seats.clone(),
                active,
                vec![artifact(1, active, Some(ManaCost::free()))],
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                test_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let cast = cast(&game, active, 1, None);
        let mut game = cast.into_state();
        for (index, seat) in seats.iter().enumerate() {
            assert_eq!(holder(&game), *seat);
            let candidate = pass(&game);
            if index + 1 == seats.len() {
                assert!(candidate.permanent_entry().is_some());
                assert_eq!(holder(candidate.state()), active);
                assert_eq!(
                    physical(candidate.state(), 1).zone(),
                    ObjectZone::Battlefield
                );
            } else {
                no_action_records(&candidate);
                assert_eq!(physical(candidate.state(), 1).zone(), ObjectZone::Stack);
            }
            game = candidate.into_state();
        }
    }
}

#[path = "permanent_casting/cast_copies.rs"]
mod cast_copies;
#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("cr-20260925")
}

#[path = "permanent_casting/keyword_permissions.rs"]
mod keyword_permissions;

#[path = "permanent_casting/cycling.rs"]
mod cycling;

#[path = "permanent_casting/cascade.rs"]
mod cascade;
#[path = "permanent_casting/escape.rs"]
mod escape;
#[path = "permanent_casting/exile_hand.rs"]
mod exile_hand;
#[path = "permanent_casting/keyword_costs.rs"]
mod keyword_costs;
#[path = "permanent_casting/madness.rs"]
mod madness;

#[path = "permanent_casting/attachments.rs"]
mod attachment_rules;

#[path = "permanent_casting/crew_saddle.rs"]
mod crew_saddle;
#[path = "permanent_casting/end_step_cast.rs"]
mod end_step_cast;
#[path = "permanent_casting/evoke.rs"]
mod evoke;
#[path = "permanent_casting/face_down.rs"]
mod face_down;

#[path = "permanent_casting/exhaust.rs"]
mod exhaust;

#[path = "permanent_casting/ninja.rs"]
mod ninja;

#[path = "permanent_casting/day_night.rs"]
mod day_night;
#[path = "permanent_casting/power_up.rs"]
mod power_up;
#[path = "permanent_casting/speed.rs"]
mod speed;

#[path = "permanent_casting/basic_land_search.rs"]
mod basic_land_search;

fn seeds() -> Vec<SeedCard> {
    let mut active_artifact = artifact(2, A, Some(ManaCost::generic(1.into())));
    active_artifact.program.activated.push(tap_ability());
    vec![
        creature(1, A, ObjectDestination::Hand, Some(cost()), 3),
        active_artifact,
        creature(3, B, ObjectDestination::Hand, Some(ManaCost::free()), 3),
        artifact(4, B, Some(ManaCost::free())),
        artifact(5, A, Some(ManaCost::free())),
        creature(6, A, ObjectDestination::Hand, None, 3),
        artifact(7, A, None),
        land(8, ObjectDestination::Hand, false),
        land(9, ObjectDestination::Battlefield { controller: A }, true),
        spell(
            10,
            A,
            spellwake_engine::program::Instruction::DealDamage(Box::new(
                spellwake_engine::toolbox::DealDamageSpec {
                    source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                    recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                        spellwake_engine::program::ValueExpr::Literal(2.into()),
                    ),
                    exile_if_dies: false,
                },
            )),
            ManaCost::one(ManaType::Red),
            BodyTarget::SingleCreature,
        ),
        spell(
            11,
            B,
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                ),
            )),
            ManaCost::specific(ManaType::Blue, 2.into()),
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
        ),
        spell(
            12,
            A,
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
            ManaCost::specific(ManaType::Blue, 2.into()),
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
        ),
        spell(
            13,
            A,
            Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ZoneDest::Hand,
            ))),
            ManaCost::one(ManaType::Blue),
            BodyTarget::SingleCreature,
        ),
        SeedCard {
            card: CardId(14),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::sorcery(
                ManaCost::one(ManaType::Green),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![spellwake_engine::program::Instruction::DealDamage(
                        Box::new(spellwake_engine::toolbox::DealDamageSpec {
                            source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                            recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                            amount: spellwake_engine::program::DamageAmountSpec::Expr(
                                spellwake_engine::program::ValueExpr::Literal(2.into()),
                            ),
                            exile_if_dies: false,
                        }),
                    )],
                ),
            ),
        },
        spell(
            15,
            B,
            Instruction::StackOp(Box::new(
                spellwake_engine::toolbox::StackOpSpec::copy_targeted_spell(),
            )),
            ManaCost::specific(ManaType::Blue, 2.into()),
            BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
        ),
        creature(
            90,
            B,
            ObjectDestination::Battlefield { controller: B },
            None,
            20,
        ),
        creature(98, A, ObjectDestination::LibraryBottom, None, 3),
        creature(99, B, ObjectDestination::LibraryBottom, None, 3),
    ]
}
