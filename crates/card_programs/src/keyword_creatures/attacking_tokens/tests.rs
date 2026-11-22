//! Exercise the actual catalogue definitions and their generated token templates.
use super::super::tests::current_catalogue;
use crate::{CURRENT_KEYWORD_PROFILE, OracleId, TEACHING_SOURCE_SHA256};
use spellwake_engine::{
    characteristics::{Color, KeywordAbility},
    combat::{AttackDeclaration, BlockStatus, CombatDecision, CombatRules},
    game::{DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const C: SeatId = SeatId(2);

fn field(controller: SeatId) -> ObjectDestination {
    ObjectDestination::Battlefield { controller }
}
fn game(players: &[SeatId], rows: &[(u64, &str, SeatId, ObjectDestination)]) -> GameState {
    let (_, vocab, registry) = current_catalogue();
    let names = std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocab.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "attacking-tokens".into(),
        "source-game".into(),
        vocab.id(),
        names.id(),
    )
    .unwrap();
    let mut cards = Vec::new();
    let mut order = Vec::new();
    let library = players.iter().enumerate().flat_map(|(i, seat)| {
        (0..3).map(move |j| {
            (
                100 + i as u64 * 3 + j,
                "Island",
                *seat,
                ObjectDestination::LibraryBottom,
            )
        })
    });
    for (n, name, seat, zone) in rows.iter().copied().chain(library) {
        let row = registry
            .coverage
            .cards
            .iter()
            .find(|r| r.required && r.name == name)
            .unwrap();
        if matches!(zone, ObjectDestination::Battlefield { .. }) {
            order.push(CardId(n));
        }
        cards.push(
            registry
                .seed(
                    &OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap(),
                    CardId(n),
                    seat,
                    zone,
                )
                .unwrap(),
        );
    }
    GameState::initialize_scheduled(
        GameId(181),
        context,
        GameSeed::with_uniform_life(
            players.to_vec(),
            A,
            cards,
            order,
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            names,
        ),
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state()
}
fn act(s: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    s.advance(GameInput {
        basis: s.basis().clone(),
        actor,
        command,
    })
    .unwrap()
    .into_state()
}
fn pass(s: &GameState) -> GameState {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
    else {
        panic!("expected priority, got {:?}", s.pending_decision())
    };
    act(s, holder, GameCommand::Pass)
}
fn reply(s: &GameState, choice: DecisionReply) -> GameState {
    let d = s.pending_decision().unwrap();
    act(
        s,
        d.chooser(),
        GameCommand::Reply {
            decision: d.id(),
            choice,
        },
    )
}
fn until(mut s: GameState, done: impl Fn(&GameState) -> bool) -> GameState {
    for _ in 0..160 {
        if done(&s) {
            return s;
        }
        s = if let Some(d) = s.pending_decision() {
            let choice = match d.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                RuleFrame::Combat(CombatDecision::Blockers(_)) => {
                    DecisionReply::DeclareBlockers(vec![])
                }
                other => panic!("unexpected decision {other:?}"),
            };
            reply(&s, choice)
        } else {
            pass(&s)
        };
    }
    panic!("requested game boundary never arrived")
}
fn attacking(s: &GameState, seat: SeatId) -> bool {
    matches!(s.pending_decision().map(|x|x.frame()),Some(RuleFrame::Combat(CombatDecision::Attackers(f))) if f.chooser()==seat)
}
fn id(s: &GameState, n: u64) -> ObjectId {
    s.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn tokens(s: &GameState) -> Vec<ObjectId> {
    s.execution()
        .window()
        .cards()
        .iter()
        .filter(|p| p.is_token() && p.zone() == ObjectZone::Battlefield)
        .map(|p| p.id())
        .collect()
}
fn attack(s: &GameState, attacker: ObjectId, defender: SeatId) -> GameState {
    reply(
        s,
        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
            attacker,
            defender,
            defending_planeswalker: None,
        }]),
    )
}
fn stack_at(s: &GameState, step: Step) -> bool {
    s.turn().unwrap().position().step == step && !s.execution().window().entries().is_empty()
}

#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn dalkovan_packbeasts_create_three_blockable_warriors_with_independent_defenders_until_next_end_step()
 {
    let mut s = game(
        &[A, B, C],
        &[
            (1, "Dalkovan Packbeasts", A, field(A)),
            (2, "Ornithopter", B, field(B)),
            (3, "Ornithopter", C, field(C)),
        ],
    );
    s = until(s, |s| attacking(s, A));
    s = attack(&s, id(&s, 1), B);
    assert_eq!(s.execution().window().entries().len(), 1);
    s = until(s, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::TimestampOrder(_))
        )
    });
    let RuleFrame::TimestampOrder(order) = s.pending_decision().unwrap().frame() else {
        unreachable!()
    };
    let order = order.members().iter().rev().map(|p| p.member).collect();
    s = reply(&s, DecisionReply::TimestampOrder(order));
    for (ordinal, defender) in [C, B, C].into_iter().enumerate() {
        assert!(tokens(&s).is_empty());
        let d = s.pending_decision().unwrap();
        let RuleFrame::TokenDefender(f) = d.frame() else {
            panic!("wanted Warrior's defender")
        };
        assert_eq!(
            f.defenders().collect::<Vec<_>>(),
            [TargetRef::Player(B), TargetRef::Player(C)]
        );
        assert_eq!(f.member().ordinal(), ordinal);
        assert!(
            s.advance(GameInput {
                basis: s.basis().clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision: d.id(),
                    choice: DecisionReply::TokenDefender(TargetRef::Player(A))
                }
            })
            .is_err()
        );
        s = reply(
            &s,
            DecisionReply::TokenDefender(TargetRef::Player(defender)),
        );
    }
    let warriors = tokens(&s);
    assert_eq!(warriors.len(), 3);
    let world = s.execution().evaluated_characteristics().unwrap();
    for (&token, defender) in warriors.iter().zip([C, B, C]) {
        let row = s.execution().combat().unwrap().attacker(token).unwrap();
        assert_eq!(row.defender(), defender);
        assert_eq!(row.block_status(), BlockStatus::NotDeclared);
        assert_eq!(
            s.execution().window().cards().get(token).unwrap().tapped(),
            Some(true)
        );
        let view = world.get(token).unwrap();
        assert_eq!(view.current_colors, [Color::Red].into());
        assert_eq!(
            view.current_creature_subtypes,
            [s.execution()
                .creature_vocabulary()
                .resolve_label("Warrior")
                .unwrap()]
            .into()
        );
        let stats = &view.power_toughness.as_ref().unwrap().derived;
        assert_eq!(
            (stats.power.clone(), stats.toughness.clone()),
            (1.into(), 1.into())
        );
    }
    for seat in [B, C] {
        s = until(
            s,
            |s| matches!(s.pending_decision().map(|p|p.frame()),Some(RuleFrame::Combat(CombatDecision::Blockers(f))) if f.chooser()==seat),
        );
        let RuleFrame::Combat(CombatDecision::Blockers(f)) = s.pending_decision().unwrap().frame()
        else {
            unreachable!()
        };
        assert!(warriors.iter().any(|token| {
            f.legal_attackers(id(&s, if seat == B { 2 } else { 3 }))
                .contains(token)
        }));
        s = reply(&s, DecisionReply::DeclareBlockers(vec![]));
    }
    s = until(s, |s| {
        s.turn().unwrap().position().step == Step::PostcombatMain
    });
    assert_eq!(tokens(&s), warriors);
    s = until(s, |s| stack_at(s, Step::End));
    assert_eq!(tokens(&s), warriors);
    assert_eq!(s.execution().window().entries().len(), 1);
    s = until(s, |s| tokens(s).is_empty());
    assert_eq!(s.turn().unwrap().position().step, Step::End);
}

#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn falcon_abomination_zombie_cannot_block_and_its_attack_sacrifices_at_end_of_combat() {
    let mut s = game(
        &[A, B],
        &[
            (1, "Falcon Abomination", A, ObjectDestination::Hand),
            (2, "Blade Instructor", B, field(B)),
            (3, "Island", A, field(A)),
            (4, "Island", A, field(A)),
            (5, "Island", A, field(A)),
        ],
    );
    s = until(s, |s| {
        s.turn().unwrap().position().step == Step::PrecombatMain
    });
    for n in 3..=5 {
        s = act(
            &s,
            A,
            GameCommand::ActivateMana {
                source: id(&s, n),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        );
    }
    s = act(
        &s,
        A,
        GameCommand::CastUntargeted {
            spell: id(&s, 1),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::Blue,
                    amount: 3.into(),
                }],
            }),
        },
    );
    s = until(s, |s| !tokens(s).is_empty());
    let zombie = tokens(&s)[0];
    let world = s.execution().evaluated_characteristics().unwrap();
    let creature = world.get(zombie).unwrap();
    assert!(creature.has_keyword(KeywordAbility::Decayed));
    assert_eq!(creature.current_colors, [Color::Black].into());
    assert_eq!(
        creature.current_creature_subtypes,
        [s.execution()
            .creature_vocabulary()
            .resolve_label("Zombie")
            .unwrap()]
        .into()
    );
    s = until(s, |s| attacking(s, B));
    s = attack(&s, id(&s, 2), A);
    s = until(s, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::Combat(CombatDecision::Blockers(_)))
        )
    });
    let RuleFrame::Combat(CombatDecision::Blockers(f)) = s.pending_decision().unwrap().frame()
    else {
        unreachable!()
    };
    assert!(f.eligible_blockers().contains(&id(&s, 1)));
    assert!(!f.eligible_blockers().contains(&zombie));
    s = reply(&s, DecisionReply::DeclareBlockers(vec![]));
    s = until(s, |s| attacking(s, A));
    s = attack(&s, zombie, B);
    assert_eq!(s.execution().window().entries().len(), 1);
    s = until(s, |s| stack_at(s, Step::EndCombat));
    assert_eq!(tokens(&s), [zombie]);
    s = until(s, |s| tokens(s).is_empty());
    assert_eq!(s.turn().unwrap().position().step, Step::EndCombat);
}

#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn dragonback_lancer_creates_one_warrior_without_a_spurious_duel_defender_choice() {
    let mut s = game(&[A, B], &[(1, "Dragonback Lancer", A, field(A))]);
    s = until(s, |s| attacking(s, A));
    s = attack(&s, id(&s, 1), B);
    s = until(s, |s| !tokens(s).is_empty());
    let token = tokens(&s)[0];
    assert_eq!(
        s.execution()
            .combat()
            .unwrap()
            .attacker(token)
            .unwrap()
            .defender(),
        B
    );
    assert_eq!(s.execution().window().entries().len(), 0);
    s = until(s, |s| stack_at(s, Step::End));
    assert_eq!(tokens(&s), [token]);
    s = until(s, |s| tokens(s).is_empty());
    assert_eq!(s.turn().unwrap().position().step, Step::End);
}
