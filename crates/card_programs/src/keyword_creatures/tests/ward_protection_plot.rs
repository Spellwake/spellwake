use super::*;
use spellwake_engine::{
    effects::{EffectError, ResolutionPaymentReply},
    game::{DecisionReply, GameError, RuleFrame},
    targets::TargetRef,
};

#[test]
#[ignore = "requires the dated Oracle catalogue and matching creature vocabulary"]
fn shivan_fire_meets_ward_and_protection_and_the_djinn_can_be_plotted() {
    let (inventory, vocabulary, registry) = crate::keyword_creatures::tests::extended_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let battlefield = ObjectDestination::Battlefield { controller: b };
    let mut seeds = vec![];
    for (id, name, owner, dest) in [
        (1, "Shivan Fire", a, ObjectDestination::Hand),
        (2, "Voice of Law", b, battlefield),
        (3, "Tomakul Honor Guard", b, battlefield),
        (4, "Djinn of Fool's Fall", a, ObjectDestination::Hand),
    ] {
        let row = inventory.cards.iter().find(|c| c.name == name).unwrap();
        let identity = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        seeds.push(
            registry
                .seed(&identity, CardId(id), owner, dest)
                .unwrap_or_else(|e| panic!("{name}: {e:?}")),
        );
    }
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        KEYWORD_GAMEPLAY_V3_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "ward-protection-plot".into(),
        "dated-card-game".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let mana = [
        (ManaType::Blue, 1),
        (ManaType::Red, 1),
        (ManaType::Colourless, 3),
    ]
    .into_iter()
    .map(|(mana_type, n)| FloatingMana {
        player: a,
        mana_type,
        amount: n.into(),
    })
    .collect();
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        seeds,
        vec![CardId(2), CardId(3)],
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let game = GameState::initialize_scheduled(
        GameId(16),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let obj = |s: &GameState, n| {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    let cast = |s: &GameState, n| GameCommand::Cast {
        spell: obj(s, 1),
        target: TargetRef::Object(obj(s, n)),
    };
    assert_eq!(
        game.advance(GameInput {
            basis: game.basis().clone(),
            actor: a,
            command: cast(&game, 2)
        })
        .unwrap_err(),
        GameError::Effect(EffectError::IllegalTarget)
    );
    let spent = ManaPayment {
        from_lots: Default::default(),
        spend: vec![
            ManaSpend {
                mana_type: ManaType::Blue,
                amount: 1.into(),
            },
            ManaSpend {
                mana_type: ManaType::Colourless,
                amount: 3.into(),
            },
        ],
    };
    let plotted = advance(
        &game,
        a,
        GameCommand::Plot {
            source: obj(&game, 4),
            payment: Some(spent),
        },
    );
    assert_eq!(plotted.execution().plotted_on(obj(&plotted, 4)), Some(1));
    let fire = advance(&plotted, a, cast(&plotted, 3));
    let next = advance(&advance(&fire, a, GameCommand::Pass), b, GameCommand::Pass);
    let decision = next.pending_decision().unwrap();
    let RuleFrame::ResolutionPayment(frame) = decision.frame() else {
        panic!("expected ward payment")
    };
    assert_eq!(frame.payer(), a);
    assert_eq!(*frame.cost(), ManaCost::generic(2.into()));
    let done = advance(
        &next,
        a,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline),
        },
    );
    assert!(done.execution().window().entries().is_empty());
    assert_eq!(
        done.execution()
            .window()
            .cards()
            .get(obj(&done, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(a)
    );
    assert_eq!(
        done.execution().battlefield_marked_damage(obj(&done, 3)),
        Some(0.into())
    );
}
