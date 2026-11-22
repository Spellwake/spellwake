use super::*;
use spellwake_engine::{
    activated::{ActivationRestriction, SourceCost},
    effects::{EffectError, LibraryChoiceKind, LibraryTopReply},
    game::{DecisionReply, GameError, RuleFrame},
    occurrence::{RulesOccurrence, RulesRecord, ZoneReason},
    stack_window::StackKind,
};

#[test]
#[ignore = "requires the dated Oracle catalogue and matching vocabulary"]
fn fleshwrither_sacrifices_at_sorcery_speed_and_transfigures_only_an_equal_value_creature() {
    let (inventory, vocab, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let seed = |name: &str, n, owner, destination| {
        let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry
            .seed(&id, CardId(n), owner, destination)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
    };
    let flesh = seed(
        "Fleshwrither",
        1,
        a,
        ObjectDestination::Battlefield { controller: a },
    );
    let [ability] = flesh.program.activated.as_slice() else {
        panic!("one printed activated ability")
    };
    assert_eq!(ability.mana_cost, fixed_cost("{1}{B}{B}").unwrap());
    assert_eq!(ability.source_cost, SourceCost::Sacrifice);
    assert_eq!(
        flesh.program.activation_restrictions.get(&ability.id),
        Some(&ActivationRestriction::Sorcery)
    );
    let activation_id = ability.id;
    let object = |g: &GameState, n| {
        g.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    let init = |step| {
        let cards = vec![
            flesh.clone(),
            seed("Arcbound Hybrid", 2, a, ObjectDestination::LibraryBottom),
            SeedCard {
                card: CardId(3),
                owner: a,
                destination: ObjectDestination::LibraryBottom,
                program: Program::artifact(Some(ManaCost::generic(4.into())), vec![]),
            },
            seed("Raging Goblin", 4, a, ObjectDestination::LibraryBottom),
            seed("Snapping Drake", 5, b, ObjectDestination::LibraryBottom),
        ];
        GameState::initialize_scheduled(
            GameId(73),
            GameContext::new(
                CURRENT_KEYWORD_PROFILE.into(),
                vocab.rules_revision().into(),
                TEACHING_SOURCE_SHA256.into(),
                "transfigure".into(),
                "fleshwrither".into(),
                vocab.id(),
                names.id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                cards,
                vec![CardId(1)],
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Black,
                    amount: 3.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                vocab.clone(),
                names.clone(),
            ),
            TurnStart { number: 1, step },
        )
        .unwrap()
        .into_state()
    };
    let pay = |amount: i64| {
        Some(ManaPayment {
            from_lots: Default::default(),
            spend: vec![ManaSpend {
                mana_type: ManaType::Black,
                amount: amount.into(),
            }],
        })
    };
    let activation = |g: &GameState, amount| GameCommand::ActivateUntargetedAbility {
        source: object(g, 1),
        ability: activation_id,
        payment: pay(amount),
    };
    let input = |g: &GameState, actor, command| GameInput {
        basis: g.basis().clone(),
        actor,
        command,
    };
    let combat = init(Step::BeginningCombat);
    assert!(matches!(
        combat.advance(input(&combat, a, activation(&combat, 3))),
        Err(GameError::Effect(EffectError::Timing(_)))
    ));
    let game = init(Step::PrecombatMain);
    assert!(game.advance(input(&game, a, activation(&game, 2))).is_err());
    assert_eq!(game.execution().mana(a, ManaType::Black), 3.into());
    let source = object(&game, 1);
    let paid = game.advance(input(&game, a, activation(&game, 3))).unwrap();
    let stack = paid.state();
    assert_eq!(stack.execution().mana(a, ManaType::Black), 0.into());
    let [entry] = stack.execution().window().entries() else {
        panic!("one activated ability")
    };
    assert!(
        matches!(entry.kind(),StackKind::ActivatedAbility{source: captured} if captured.id()==source)
    );
    assert_eq!(
        stack
            .execution()
            .window()
            .cards()
            .get(object(stack, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(a)
    );
    let passed = advance(stack, a, GameCommand::Pass);
    let pending = advance(&passed, b, GameCommand::Pass);
    let pending = pending
        .supply_entropy(pending.basis(), vec![0, 0])
        .unwrap()
        .into_state();
    let decision = pending.pending_decision().unwrap();
    let RuleFrame::LibraryTop(frame) = decision.frame() else {
        panic!("private creature search")
    };
    assert_eq!(frame.kind(), LibraryChoiceKind::Search);
    let hybrid = object(&pending, 2);
    assert_eq!(frame.eligible_results(), &[hybrid]);
    let request = |actor, selected| {
        input(
            &pending,
            actor,
            GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
            },
        )
    };
    for n in [3, 4, 5] {
        assert!(
            pending
                .advance(request(a, Some(object(&pending, n))))
                .is_err()
        );
    }
    assert!(pending.advance(request(b, Some(hybrid))).is_err());
    for take in [true, false] {
        let done = pending.advance(request(a, take.then_some(hybrid))).unwrap();
        let next = done.state();
        let [shuffle] = done.shuffle_journal() else {
            panic!("exactly one shuffle")
        };
        assert_eq!(shuffle.player, a);
        assert_eq!(shuffle.before.len(), if take { 2 } else { 3 });
        assert_eq!(
            shuffle.entropy_end - shuffle.entropy_start,
            shuffle.before.len() - 1
        );
        assert_ne!(shuffle.before, shuffle.after);
        assert_eq!(shuffle.after, next.execution().window().cards().library(a));
        assert!(next.search_reveal().is_none());
        assert!(next.execution().window().entries().is_empty());
        let id = object(next, 2);
        let current = next.execution().window().cards().get(id).unwrap();
        if take {
            assert_ne!(id, hybrid);
            assert_eq!(
                next.execution()
                    .power_toughness_counters(id)
                    .unwrap()
                    .plus_one,
                2.into()
            );
            assert_eq!(current.zone(), ObjectZone::Battlefield);
            assert_eq!(current.controller(), Some(a));
            assert!(
                next.execution()
                    .current_characteristics(id)
                    .unwrap()
                    .unwrap()
                    .has_keyword(KeywordAbility::Haste)
            );
            assert!(done.rules_journal().records.iter().any(|record| {
                let RulesRecord::Event(event)=record else { return false; };
                event.occurrences.iter().any(|row| matches!(&row.value, RulesOccurrence::ZoneChange(change) if change.reason==ZoneReason::TransfigureEntry && change.after.object.id()==id))
                    && event.occurrences.iter().any(|row|matches!(&row.value, RulesOccurrence::CountersPlaced{object,added,..} if *object==id && added.plus_one==2.into()))
            }));
        } else {
            assert_eq!(id, hybrid);
            assert_eq!(current.zone(), ObjectZone::Library(a));
        }
    }
}
