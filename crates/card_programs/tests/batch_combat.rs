//! Source-bound paid casts and subsequent combat through the scheduled game.
#[path = "../../engine/test_support/creature_vocabulary.rs"]
mod vocabulary;

use spellwake_card_programs::{CURRENT_KEYWORD_PROFILE, OracleId, Registry};
use spellwake_content::{Inventory, snapshot};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use spellwake_contracts::names::NameRules;
use spellwake_engine::{
    characteristics::{
        Color, ControlRecipient, KeywordAbility, ObjectCharacteristics, PowerToughness,
        ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec,
        TypeOperation,
    },
    combat::{
        AttackDeclaration, BlockDeclaration, BlockStatus, CombatDamageAssignment, CombatDecision,
        CombatError, CombatRules,
    },
    damage::{DamageOutcome, DamageRecipient, DamageSourceId},
    effects::{EffectError, SeedCard},
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, RuleFrame,
    },
    mana::{FloatingMana, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, GameObject, ObjectDestination, ObjectZone},
    priority::{PriorityPhase, SeatId},
    program::{BodyTarget, CardType, ExecutableBody, Instruction, Program},
    setup::GameSeed,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

const ALICE: SeatId = SeatId(0);
const BOB: SeatId = SeatId(1);
const SUBJECT: u64 = 1;
const KNIGHTHOOD: &str = "22f008b3-c263-4dcc-855f-403da9ca4cd9";
const STORMCLOUD_SPIRIT: &str = "0dc8bdbb-996b-47d7-a5e5-7bdb6d9280fe";
const DEFIANT_ELF: &str = "6faa29c4-582a-42d5-b80c-4dc0bd738493";

fn current_catalogue() -> &'static (Inventory, Arc<CreatureTypeVocabulary>, Registry) {
    static SOURCE: OnceLock<(Inventory, Arc<CreatureTypeVocabulary>, Registry)> = OnceLock::new();
    SOURCE.get_or_init(|| {
        let input =
            |name| PathBuf::from(std::env::var_os(name).expect("explicit saved source input"));
        let vocabulary =
            vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
        let inventory = snapshot::load(
            &input("SPELLWAKE_CATALOGUE_METADATA"),
            &input("SPELLWAKE_CATALOGUE_INPUT"),
        )
        .unwrap();
        let registry = Registry::from_current_keyword_inventory(&inventory, &vocabulary).unwrap();
        (inventory, vocabulary, registry)
    })
}

fn setup(oracle_id: &str, mana: &[(ManaType, i64)], mut cards: Vec<SeedCard>) -> GameState {
    let (inventory, vocabulary, registry) = current_catalogue();
    let oracle_id = OracleId::try_from(oracle_id.to_owned()).unwrap();
    cards.push(
        registry
            .seed(&oracle_id, CardId(SUBJECT), ALICE, ObjectDestination::Hand)
            .unwrap(),
    );
    for (id, owner) in [(100, ALICE), (101, BOB)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination: ObjectDestination::LibraryBottom,
            program: Program::artifact(None, vec![]),
        });
    }
    let battlefield_order = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let names = Arc::new(NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        inventory.compressed_sha256.clone(),
        format!("keyword-registry-{}", registry.coverage().registry_version),
        "source-bound-combat".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    GameState::initialize_scheduled(
        GameId(1),
        context,
        GameSeed::with_uniform_life(
            vec![ALICE, BOB],
            ALICE,
            cards,
            battlefield_order,
            mana.iter()
                .map(|&(mana_type, amount)| FloatingMana {
                    player: ALICE,
                    mana_type,
                    amount: amount.into(),
                })
                .collect(),
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            Arc::clone(vocabulary),
            names,
        ),
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state()
}

fn battlefield(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller: owner },
        program,
    }
}

fn creature(id: u64, owner: SeatId, size: i64, keywords: &[KeywordAbility]) -> SeedCard {
    battlefield(
        id,
        owner,
        Program::creature(None, size.into(), size.into(), Default::default())
            .with_printed_keywords(keywords.iter().copied()),
    )
}

fn instant(id: u64, owner: SeatId, target: BodyTarget, instruction: Instruction) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(target, vec![instruction]),
        ),
    }
}

fn continuous(part: ResolvedPartSpec, duration: ResolvedDuration) -> Instruction {
    Instruction::CreateContinuousEffect {
        parts: ResolvedPartsSpec(vec![part]),
        duration,
    }
}

fn object(game: &GameState, card: u64) -> &GameObject {
    let cards = game.execution().window().cards();
    cards
        .get(cards.current_identity(CardId(card)).unwrap())
        .unwrap()
}

fn sheet(game: &GameState, card: u64) -> ObjectCharacteristics {
    game.execution()
        .current_characteristics(object(game, card).id())
        .unwrap()
        .unwrap()
}

fn advance(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .unwrap()
}

fn pass(game: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected priority");
    };
    advance(game, holder, GameCommand::Pass)
}

fn round(game: &GameState) -> NativeCandidate {
    pass(pass(game).state())
}

fn cast_subject(game: &GameState, mana: &[(ManaType, i64)]) -> GameState {
    let cast = advance(
        game,
        ALICE,
        GameCommand::CastUntargeted {
            spell: object(game, SUBJECT).id(),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: mana
                    .iter()
                    .map(|&(mana_type, amount)| ManaSpend {
                        mana_type,
                        amount: amount.into(),
                    })
                    .collect(),
            }),
        },
    );
    assert_eq!(object(cast.state(), SUBJECT).zone(), ObjectZone::Stack);
    for &(mana_type, _) in mana {
        assert_eq!(cast.state().execution().mana(ALICE, mana_type), 0.into());
    }
    let resolved = round(cast.state());
    let entry = resolved.permanent_entry().unwrap();
    assert_eq!(entry.after.card_id(), Some(CardId(SUBJECT)));
    assert_eq!(entry.after.zone(), ObjectZone::Battlefield);
    resolved.into_state()
}

fn cast_on(game: &GameState, actor: SeatId, spell: u64, target: u64) -> NativeCandidate {
    let priority = match game.execution().window().phase() {
        WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == actor => game.clone(),
        WindowPhase::Priority(PriorityPhase::Ready { .. }) => pass(game).into_state(),
        phase => panic!("expected priority, received {phase:?}"),
    };
    let cast = advance(
        &priority,
        actor,
        GameCommand::Cast {
            spell: object(&priority, spell).id(),
            target: TargetRef::Object(object(&priority, target).id()),
        },
    );
    round(cast.state())
}

fn reply_input(game: &GameState, choice: DecisionReply) -> GameInput {
    let pending = game.pending_decision().unwrap();
    GameInput {
        basis: game.basis().clone(),
        actor: pending.chooser(),
        command: GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    }
}

fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    game.advance(reply_input(game, choice)).unwrap()
}

fn expect_combat_error(game: &GameState, choice: DecisionReply, error: CombatError) {
    let before = game.clone();
    assert_eq!(
        game.advance(reply_input(game, choice)),
        Err(GameError::Effect(EffectError::Combat(error)))
    );
    assert_eq!(game, &before);
}

fn reach(game: &GameState, turn: u64, step: Step) -> GameState {
    let mut state = game.clone();
    for _ in 0..24 {
        let position = state.turn().unwrap().position();
        if position.number == turn && position.step == step {
            return state;
        }
        if let Some(pending) = state.pending_decision() {
            assert!(matches!(
                pending.frame(),
                RuleFrame::Combat(CombatDecision::Attackers(_))
            ));
            assert!(position.number < turn);
            state = reply(&state, DecisionReply::DeclareAttackers(vec![])).into_state();
        } else {
            state = round(&state).into_state();
        }
    }
    panic!("turn {turn} did not reach {step:?}");
}

fn attack(game: &GameState, turn: u64, attacker: u64, defender: SeatId) -> GameState {
    let state = reach(game, turn, Step::DeclareAttackers);
    let declared = reply(
        &state,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker: object(&state, attacker).id(),
            defender,
            defending_planeswalker: None,
        }]),
    );
    round(declared.state()).into_state()
}

fn block_choice(game: &GameState, blocker: u64, attacker: u64) -> DecisionReply {
    DecisionReply::DeclareBlockers(vec![BlockDeclaration {
        blocker: object(game, blocker).id(),
        attacker: object(game, attacker).id(),
    }])
}

fn block(game: &GameState, blocker: u64, attacker: u64) -> GameState {
    reply(game, block_choice(game, blocker, attacker)).into_state()
}

fn marked(game: &GameState, card: u64, damage: i64) {
    assert_eq!(
        game.execution()
            .battlefield_marked_damage(object(game, card).id()),
        Some(damage.into())
    );
}

const KNIGHTHOOD_MANA: &[(ManaType, i64)] = &[(ManaType::Colourless, 2), (ManaType::White, 1)];

#[test]
#[ignore = "requires the pinned catalogue and source-qualified vocabulary"]
fn knighthood_tracks_current_controller_types_and_source_departure() {
    let gain = continuous(
        ResolvedPartSpec::Control(ControlRecipient::ResolvingController),
        ResolvedDuration::NoStatedDuration,
    );
    let type_change = continuous(
        ResolvedPartSpec::Type(TypeChangeSpec {
            types: TypeOperation::Set([CardType::Artifact].into()),
            supertypes: TypeOperation::Keep,
            land_subtypes: SubtypeOperation::Keep,
            subtypes: SubtypeOperation::Keep,
        }),
        ResolvedDuration::UntilEndOfTurn,
    );
    let game = setup(
        KNIGHTHOOD,
        KNIGHTHOOD_MANA,
        vec![
            creature(2, ALICE, 2, &[]),
            creature(3, BOB, 2, &[]),
            creature(4, ALICE, 2, &[]),
            instant(5, ALICE, BodyTarget::SinglePermanent, gain.clone()),
            instant(6, BOB, BodyTarget::SinglePermanent, gain.clone()),
            instant(7, ALICE, BodyTarget::SinglePermanent, type_change),
            instant(8, BOB, BodyTarget::SinglePermanent, gain),
            instant(
                9,
                ALICE,
                BodyTarget::SinglePermanent,
                Instruction::ReturnToOwnersHand,
            ),
        ],
    );
    let game = cast_subject(&game, KNIGHTHOOD_MANA);
    let has_first =
        |game: &GameState, card| sheet(game, card).has_keyword(KeywordAbility::FirstStrike);
    assert!(has_first(&game, 2));
    assert!(!has_first(&game, 3));
    assert!(!has_first(&game, SUBJECT));

    let gained = cast_on(&game, ALICE, 5, 3).into_state();
    assert_eq!(object(&gained, 3).controller(), Some(ALICE));
    assert!(has_first(&gained, 3));

    let lost = cast_on(&gained, BOB, 6, 2).into_state();
    assert_eq!(object(&lost, 2).controller(), Some(BOB));
    assert!(!has_first(&lost, 2));

    let typed = cast_on(&lost, ALICE, 7, 4).into_state();
    assert_eq!(sheet(&typed, 4).current_types, [CardType::Artifact].into());
    assert!(!has_first(&typed, 4));

    let stolen = cast_on(&typed, BOB, 8, SUBJECT).into_state();
    assert_eq!(object(&stolen, SUBJECT).controller(), Some(BOB));
    assert!(has_first(&stolen, 2));
    assert!(!has_first(&stolen, 3));

    let departed = cast_on(&stolen, ALICE, 9, SUBJECT).into_state();
    assert_eq!(object(&departed, SUBJECT).zone(), ObjectZone::Hand(ALICE));
    assert!(!has_first(&departed, 2));
}

#[test]
#[ignore = "requires the pinned catalogue and source-qualified vocabulary"]
fn knighthood_wins_equal_combat_as_attacker_or_blocker_in_first_damage_step() {
    for (turn, attacker, blocker, defender) in [(3, 2, 3, BOB), (2, 3, 2, ALICE)] {
        let game = setup(
            KNIGHTHOOD,
            KNIGHTHOOD_MANA,
            vec![creature(2, ALICE, 2, &[]), creature(3, BOB, 2, &[])],
        );
        let cast = cast_subject(&game, KNIGHTHOOD_MANA);
        let blocked = block(&attack(&cast, turn, attacker, defender), blocker, attacker);
        let early = round(&blocked).into_state();
        let position = early.turn().unwrap().position();
        assert_eq!(position.step, Step::CombatDamage);
        assert!(!position.additional_damage);
        assert_eq!(object(&early, 3).zone(), ObjectZone::Graveyard(BOB));
        marked(&early, 2, 0);
        let late = round(&early);
        assert!(late.state().turn().unwrap().position().additional_damage);
        assert!(late.damage_journal().applied.is_empty());
        marked(late.state(), 2, 0);
    }
}

#[test]
#[ignore = "requires the pinned catalogue and source-qualified vocabulary"]
fn knighthood_leaving_between_damage_steps_does_not_let_its_attacker_strike_twice() {
    let game = setup(
        KNIGHTHOOD,
        KNIGHTHOOD_MANA,
        vec![
            creature(2, ALICE, 2, &[]),
            creature(3, BOB, 3, &[]),
            instant(
                4,
                ALICE,
                BodyTarget::SinglePermanent,
                Instruction::ReturnToOwnersHand,
            ),
        ],
    );
    let cast = cast_subject(&game, KNIGHTHOOD_MANA);
    let blocked = block(&attack(&cast, 3, 2, BOB), 3, 2);
    let early = round(&blocked).into_state();
    assert!(!early.turn().unwrap().position().additional_damage);
    marked(&early, 2, 0);
    marked(&early, 3, 2);

    let departed = cast_on(&early, ALICE, 4, SUBJECT).into_state();
    assert_eq!(object(&departed, SUBJECT).zone(), ObjectZone::Hand(ALICE));
    assert!(!sheet(&departed, 2).has_keyword(KeywordAbility::FirstStrike));
    let late = round(&departed);
    assert!(late.state().turn().unwrap().position().additional_damage);
    assert_eq!(object(late.state(), 2).zone(), ObjectZone::Graveyard(ALICE));
    marked(late.state(), 3, 2);
    let applied = &late.damage_journal().applied;
    assert_eq!(applied.len(), 1);
    assert_eq!(
        applied[0].source.id(),
        DamageSourceId::Object(object(&departed, 3).id())
    );
    assert_eq!(applied[0].amount, 3.into());
}

#[test]
#[ignore = "requires the pinned catalogue and source-qualified vocabulary"]
fn stormcloud_spirit_flies_over_ground_but_blocks_ground_and_can_be_blocked_by_flying_or_reach() {
    let mana = &[(ManaType::Colourless, 3), (ManaType::Blue, 2)];
    let game = setup(
        STORMCLOUD_SPIRIT,
        mana,
        vec![
            creature(2, BOB, 1, &[]),
            creature(3, BOB, 1, &[KeywordAbility::Flying]),
            creature(4, BOB, 1, &[KeywordAbility::Reach]),
        ],
    );
    let game = cast_subject(&game, mana);
    let spirit = sheet(&game, SUBJECT);
    assert_eq!(spirit.current_colors, [Color::Blue].into());
    assert_eq!(
        spirit.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Spirit").unwrap()].into()
    );
    assert_eq!(
        spirit.power_toughness.as_ref().unwrap().derived,
        PowerToughness {
            power: 4.into(),
            toughness: 4.into()
        }
    );
    assert!(spirit.has_keyword(KeywordAbility::Flying));

    let attacking = attack(&game, 3, SUBJECT, BOB);
    expect_combat_error(
        &attacking,
        block_choice(&attacking, 2, SUBJECT),
        CombatError::IllegalBlocker,
    );
    for legal in [3, 4] {
        let accepted = block(&attacking, legal, SUBJECT);
        assert_eq!(accepted.execution().combat().unwrap().blockers().count(), 1);
        let damage = round(&accepted).into_state();
        assert_eq!(object(&damage, legal).zone(), ObjectZone::Graveyard(BOB));
        marked(&damage, SUBJECT, 1);
    }

    let defending = block(&attack(&game, 2, 2, ALICE), SUBJECT, 2);
    let damage = round(&defending).into_state();
    assert_eq!(object(&damage, 2).zone(), ObjectZone::Graveyard(BOB));
    marked(&damage, SUBJECT, 1);
}

#[test]
#[ignore = "requires the pinned catalogue and source-qualified vocabulary"]
fn defiant_elf_tramples_after_lethal_even_with_prevention_and_when_all_blockers_leave() {
    let mana = &[(ManaType::Green, 1)];
    let game = setup(
        DEFIANT_ELF,
        mana,
        vec![
            creature(2, BOB, 1, &[]),
            instant(
                3,
                ALICE,
                BodyTarget::SingleCreature,
                continuous(
                    ResolvedPartSpec::ModifyPT(PowerToughness {
                        power: 2.into(),
                        toughness: 2.into(),
                    }),
                    ResolvedDuration::UntilEndOfTurn,
                ),
            ),
            instant(
                4,
                ALICE,
                BodyTarget::SingleCreature,
                Instruction::CreatePreventionShield { amount: 1.into() },
            ),
            instant(
                5,
                ALICE,
                BodyTarget::SinglePermanent,
                Instruction::ReturnToOwnersHand,
            ),
        ],
    );
    let game = cast_subject(&game, mana);
    let elf = sheet(&game, SUBJECT);
    assert_eq!(elf.current_colors, [Color::Green].into());
    assert_eq!(
        elf.current_creature_subtypes,
        [current_catalogue().1.resolve_label("Elf").unwrap()].into(),
    );
    assert_eq!(
        elf.power_toughness.as_ref().unwrap().derived,
        PowerToughness {
            power: 1.into(),
            toughness: 1.into()
        }
    );
    assert!(elf.has_keyword(KeywordAbility::Trample));

    let third_main = reach(&game, 3, Step::PrecombatMain);
    let boosted = cast_on(&third_main, ALICE, 3, SUBJECT).into_state();
    assert_eq!(
        sheet(&boosted, SUBJECT).power_toughness.unwrap().derived,
        PowerToughness {
            power: 3.into(),
            toughness: 3.into()
        }
    );
    let blocked = block(&attack(&boosted, 3, SUBJECT, BOB), 2, SUBJECT);
    let id = object(&blocked, SUBJECT).id();
    let blocker = object(&blocked, 2).id();
    let allocation = |target, amount: i64| CombatDamageAssignment {
        source: id,
        target,
        amount: amount.into(),
    };
    let player = DamageRecipient::Player(BOB);
    let legal_assignment = || {
        DecisionReply::AssignCombatDamage(vec![
            allocation(DamageRecipient::Creature(blocker), 1),
            allocation(player, 2),
        ])
    };
    for prevented in [false, true] {
        let game = if prevented {
            cast_on(&blocked, ALICE, 4, 2).into_state()
        } else {
            blocked.clone()
        };
        let assigning = round(&game).into_state();
        let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
            assigning.pending_decision().unwrap().frame()
        else {
            panic!("expected trample assignment");
        };
        assert_eq!(
            frame.domains()[0].trample_lethal().get(&blocker),
            Some(&1.into())
        );
        expect_combat_error(
            &assigning,
            DecisionReply::AssignCombatDamage(vec![allocation(player, 3)]),
            CombatError::InsufficientTrampleAssignment,
        );
        let done = reply(&assigning, legal_assignment());
        assert_eq!(done.state().execution().life_total(BOB), Some(&18.into()));
        marked(done.state(), SUBJECT, 1);
        if prevented {
            assert_eq!(object(done.state(), 2).zone(), ObjectZone::Battlefield);
            marked(done.state(), 2, 0);
            assert!(
                done.damage_journal()
                    .completions
                    .iter()
                    .any(|completion| completion.outcome == DamageOutcome::FullyPrevented)
            );
        } else {
            assert_eq!(object(done.state(), 2).zone(), ObjectZone::Graveyard(BOB));
        }
    }

    let unopposed = cast_on(&blocked, ALICE, 5, 2).into_state();
    assert_eq!(object(&unopposed, 2).zone(), ObjectZone::Hand(BOB));
    let combat = unopposed.execution().combat().unwrap();
    assert_eq!(combat.blockers().count(), 0);
    assert_eq!(
        combat.attacker(id).unwrap().block_status(),
        BlockStatus::Blocked
    );
    let done = round(&unopposed).into_state();
    assert_eq!(done.execution().life_total(BOB), Some(&17.into()));
    marked(&done, SUBJECT, 0);
}
