use super::*;
use spellwake_engine::{
    program::{BodyTarget, ExecutableBody},
    targets::TargetRef,
};

#[test]
#[ignore = "requires the dated catalogue and source-qualified creature vocabulary"]
fn skyreach_manta_suntouched_myr_and_scab_clan_mauler_enter_with_the_paid_and_dealt_counters() {
    let (inventory, vocabulary, registry) = fourth_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let mut seeds = vec![];
    for (id, name) in [
        (1, "Skyreach Manta"),
        (2, "Suntouched Myr"),
        (3, "Scab-Clan Mauler"),
        (4, "Bloodscale Prowler"),
    ] {
        let row = inventory.cards.iter().find(|row| row.name == name).unwrap();
        let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        seeds.push(
            registry
                .seed(&oracle, CardId(id), a, ObjectDestination::Hand)
                .unwrap(),
        );
    }
    // A controlled damage source establishes the turn history for the two named bloodthirst cards.
    seeds.push(SeedCard {
        card: CardId(5),
        owner: a,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::deal_damage(
                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        spellwake_engine::program::ValueExpr::literal(2),
                        false,
                    ),
                ],
            ),
        ),
    });
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "entry-counters".into(),
        "source-games".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    use ManaType::{Black, Blue, Colourless, Green, Red, White};
    let mana = [
        (White, 1),
        (Blue, 2),
        (Black, 1),
        (Red, 2),
        (Green, 2),
        (Colourless, 5),
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
        vec![],
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let mut game = GameState::initialize_scheduled(
        GameId(18),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let object = |s: &GameState, n| {
        s.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    let resolve = |s: &GameState| advance(&advance(s, a, GameCommand::Pass), b, GameCommand::Pass);
    let damage = GameCommand::Cast {
        spell: object(&game, 5),
        target: TargetRef::Player(b),
    };
    game = resolve(&advance(&game, a, damage));
    for (n, spend, expected) in [
        (
            1,
            vec![(White, 1), (Blue, 1), (Black, 1), (Colourless, 2)],
            3,
        ),
        (2, vec![(Blue, 1), (Green, 1), (Colourless, 1)], 2),
        (3, vec![(Red, 1), (Green, 1)], 2),
        (4, vec![(Colourless, 2), (Red, 1)], 1),
    ] {
        let command = GameCommand::CastUntargeted {
            spell: object(&game, n),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: spend
                    .into_iter()
                    .map(|(mana_type, amount): (ManaType, i64)| ManaSpend {
                        mana_type,
                        amount: amount.into(),
                    })
                    .collect(),
            }),
        };
        game = resolve(&advance(&game, a, command));
        let permanent = object(&game, n);
        assert_eq!(
            game.execution()
                .window()
                .cards()
                .get(permanent)
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
        assert_eq!(
            game.execution()
                .power_toughness_counters(permanent)
                .unwrap()
                .plus_one,
            expected.into()
        );
    }
}
