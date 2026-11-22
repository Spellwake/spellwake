use super::*;
use spellwake_engine::{
    face_down::FaceDownKind,
    game::DecisionReply,
    mana::{ManaAbilityId, ManaAbilityRef},
    program::CardType,
    targets::TargetRef,
};
#[test]
#[ignore = "requires the dated catalogue and qualified vocabulary"]
fn named_morph_megamorph_disguise_and_land_preserve_identity_and_resolve_real_triggers() {
    let (inventory, vocabulary, registry) = fourth_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = [
        "Scornful Egotist",
        "Marsh Hulk",
        "Defenestrated Phantom",
        "Zoetic Cavern",
        "Echo Tracer",
        "Witness of the Ages",
    ];
    let mut cards = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let row = inventory
            .cards
            .iter()
            .find(|row| &row.name == name)
            .unwrap();
        let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        cards.push(
            registry
                .seed(&oracle, CardId(i as u64 + 1), a, ObjectDestination::Hand)
                .unwrap_or_else(|err| panic!("{name}: {err:?}")),
        );
    }
    let rules = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "face-down".into(),
        "named-game".into(),
        vocabulary.id(),
        rules.id(),
    )
    .unwrap();
    let mana = ManaType::ALL
        .into_iter()
        .map(|mana_type| FloatingMana {
            player: a,
            mana_type,
            amount: 40.into(),
        })
        .collect();
    let mut game = GameState::initialize_scheduled(
        GameId(19),
        context,
        GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![],
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            rules,
        ),
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let obj = |game: &GameState, n| {
        game.execution()
            .window()
            .cards()
            .current_identity(CardId(n))
            .unwrap()
    };
    let pay = |items: &[(ManaType, i64)]| {
        Some(ManaPayment {
            from_lots: Default::default(),
            spend: items
                .iter()
                .map(|&(mana_type, n)| ManaSpend {
                    mana_type,
                    amount: n.into(),
                })
                .collect(),
        })
    };
    let pass = |s: &GameState| {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
        else {
            panic!("expected priority")
        };
        advance(s, holder, GameCommand::Pass)
    };
    let resolve = |s: &GameState| pass(&pass(s));
    for id in [1, 2, 3, 4, 6, 5] {
        let kind = if id == 3 {
            FaceDownKind::Disguise
        } else {
            FaceDownKind::Morph
        };
        game = advance(
            &game,
            a,
            GameCommand::CastFaceDown {
                spell: obj(&game, id),
                kind,
                payment: pay(&[(ManaType::Colourless, 3)]),
            },
        );
        let check_hidden = |s: &GameState| {
            let object = obj(s, id);
            let hidden = s
                .execution()
                .current_characteristics(object)
                .unwrap()
                .unwrap();
            assert_eq!(hidden.current_types, BTreeSet::from([CardType::Creature]));
            assert!(
                hidden.current_names.is_empty()
                    && hidden.current_colors.is_empty()
                    && hidden.current_creature_subtypes.is_empty()
            );
            assert_eq!(hidden.power_toughness.unwrap().derived.power, 2.into());
            assert_eq!(
                hidden.printed_text.triggered.iter().any(|t| t.condition
                    == spellwake_engine::program::TriggerSpec::new(
                        spellwake_engine::program::TriggerEvent::Targeted {
                            subject: spellwake_engine::program::ObjectFilter::self_object(),
                            by_opponent: true,
                            spell_only: false,
                            instant_or_sorcery_only: false
                        }
                    )),
                id == 3
            );
            assert!(hidden.abilities.face_up.is_empty());
            assert!(
                s.execution()
                    .copiable_program(object)
                    .unwrap()
                    .face_up_abilities
                    .is_empty()
            );
        };
        check_hidden(&game);
        game = resolve(&game);
        check_hidden(&game);
        let permanent = obj(&game, id);
        let who = game
            .execution()
            .face_up_preview(permanent)
            .unwrap()
            .unwrap();
        assert!(
            who.current_names
                .iter()
                .any(|name| name.as_str() == names[id as usize - 1])
        );
        let price = match id {
            1 => vec![(ManaType::Blue, 1)],
            2 => vec![(ManaType::Colourless, 6), (ManaType::Black, 1)],
            3 => vec![(ManaType::Colourless, 4), (ManaType::White, 1)],
            4 => vec![(ManaType::Colourless, 2)],
            5 => vec![(ManaType::Colourless, 2), (ManaType::Blue, 1)],
            _ => vec![(ManaType::Colourless, 5)],
        };
        game = advance(
            &game,
            a,
            GameCommand::TurnFaceUp {
                permanent,
                ability: 0,
                payment: pay(&price),
            },
        );
        assert_eq!(obj(&game, id), permanent);
        let current = game
            .execution()
            .current_characteristics(permanent)
            .unwrap()
            .unwrap();
        assert!(
            current
                .current_names
                .iter()
                .any(|name| name.as_str() == names[id as usize - 1])
        );
        if id == 5 {
            let decision = game
                .pending_decision()
                .expect("Echo Tracer targets on stacking its trigger");
            game = advance(
                &game,
                a,
                GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::TriggerTarget(TargetRef::Object(obj(&game, 1))),
                },
            );
            game = resolve(&game);
            assert_eq!(
                game.execution()
                    .window()
                    .cards()
                    .get(obj(&game, 1))
                    .unwrap()
                    .zone(),
                ObjectZone::Hand(a)
            );
        } else {
            assert!(game.execution().window().entries().is_empty());
            match id {
                2 => {
                    assert_eq!(
                        game.execution()
                            .power_toughness_counters(permanent)
                            .unwrap()
                            .plus_one,
                        1.into()
                    );
                    assert_eq!(current.power_toughness.unwrap().derived.power, 5.into());
                }
                3 => assert!(current.has_keyword(KeywordAbility::Flying)),
                4 => {
                    assert_eq!(current.current_types, BTreeSet::from([CardType::Land]));
                    let before = game.execution().mana(a, ManaType::Colourless);
                    game = advance(
                        &game,
                        a,
                        GameCommand::ActivateMana {
                            source: permanent,
                            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
                        },
                    );
                    assert_eq!(
                        game.execution().mana(a, ManaType::Colourless),
                        before.plus(&1.into())
                    );
                    assert_eq!(game.turn().unwrap().lands_played(), &0.into());
                }
                6 => assert!(current.current_types.contains(&CardType::Artifact)),
                _ => assert_eq!(current.power_toughness.unwrap().derived.power, 1.into()),
            }
        }
    }
}
