//! Independently resolve both choices and the source-departed fallback at game priority.
use super::*;
use spellwake_engine::{
    effects::OptionalEffectChoice,
    game::{DecisionReply, RuleFrame},
    object_identity::ObjectId,
    program::{BodyTarget, ExecutableBody, TokenKeyword},
    targets::TargetRef,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn object(game: &GameState, n: u64) -> ObjectId {
    game.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn resolve(game: &GameState) -> GameState {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("priority")
    };
    let other = if holder == A { B } else { A };
    advance(
        &advance(game, holder, GameCommand::Pass),
        other,
        GameCommand::Pass,
    )
}
fn choose(game: &GameState, choice: DecisionReply) -> GameState {
    let frame = game.pending_decision().unwrap();
    advance(
        game,
        frame.chooser(),
        GameCommand::Reply {
            decision: frame.id(),
            choice,
        },
    )
}
fn verify_tokens(game: &GameState, keyword: TokenKeyword, amount: usize) {
    let (subtype, colors, types, keywords) = match keyword {
        TokenKeyword::Afterlife => (
            "Spirit",
            BTreeSet::from([Color::White, Color::Black]),
            BTreeSet::from([CardType::Creature]),
            BTreeSet::from([KeywordAbility::Flying]),
        ),
        TokenKeyword::Fabricate => (
            "Servo",
            BTreeSet::new(),
            BTreeSet::from([CardType::Creature, CardType::Artifact]),
            BTreeSet::new(),
        ),
    };
    let evaluated = game.execution().evaluated_characteristics().unwrap();
    let tokens = game
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), amount);
    for token in tokens {
        let view = evaluated.get(token.id()).unwrap();
        assert_eq!(token.controller(), Some(A));
        assert_eq!(token.owner(), A);
        assert_eq!(view.current_types, types);
        assert_eq!(view.current_colors, colors);
        assert_eq!(view.keywords, keywords);
        assert_eq!(
            view.current_creature_subtypes,
            BTreeSet::from([game
                .execution()
                .creature_vocabulary()
                .resolve_label(subtype)
                .unwrap()])
        );
        assert_eq!(
            (
                view.power_toughness.as_ref().unwrap().derived.power.clone(),
                view.power_toughness
                    .as_ref()
                    .unwrap()
                    .derived
                    .toughness
                    .clone()
            ),
            (1.into(), 1.into())
        );
    }
}
fn timestamps(game: &GameState) -> GameState {
    let RuleFrame::TimestampOrder(f) = game.pending_decision().unwrap().frame() else {
        panic!("expected simultaneous token order")
    };
    choose(
        game,
        DecisionReply::TimestampOrder(f.members().iter().rev().map(|m| m.member).collect()),
    )
}

fn scenarios(vocabulary: Arc<CreatureTypeVocabulary>, afterlife: Program, fabricate: Program) {
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        KEYWORD_GAMEPLAY_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "keyword-gameplay-registry13".into(),
        "fabricate-afterlife".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let instant = |instruction| {
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SINGLE_PERMANENT, vec![instruction]),
        )
    };
    let cards = vec![
        SeedCard {
            card: CardId(1),
            owner: B,
            destination: ObjectDestination::Battlefield { controller: A },
            program: afterlife,
        },
        SeedCard {
            card: CardId(2),
            owner: A,
            destination: ObjectDestination::Hand,
            program: fabricate,
        },
        SeedCard {
            card: CardId(3),
            owner: A,
            destination: ObjectDestination::Hand,
            program: instant(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                    spellwake_engine::toolbox::ZoneOpSpec::move_all(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::toolbox::ZoneDest::Graveyard {
                            semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                allow_regenerate: true,
                            },
                        },
                    ),
                ),
            ),
        },
        SeedCard {
            card: CardId(4),
            owner: A,
            destination: ObjectDestination::Hand,
            program: instant(
                crate::keyword_creatures::oracle_lowering::toolbox_emit::return_to_owners_hand(),
            ),
        },
    ];
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1)],
        vec![FloatingMana {
            player: A,
            mana_type: ManaType::White,
            amount: 4.into(),
        }],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let game = GameState::initialize_scheduled(
        GameId(44),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let cast_at = |s: &GameState, n, target| {
        advance(
            s,
            A,
            GameCommand::Cast {
                spell: object(s, n),
                target: TargetRef::Object(object(s, target)),
            },
        )
    };
    let left = resolve(&cast_at(&game, 4, 1));
    assert!(left.execution().window().entries().is_empty());
    let dead = resolve(&cast_at(&game, 3, 1));
    assert_eq!(
        dead.execution()
            .window()
            .cards()
            .get(object(&dead, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(B)
    );
    assert_eq!(dead.execution().window().entries().len(), 1);
    assert_eq!(dead.execution().window().entries()[0].controller(), A);
    verify_tokens(&dead, TokenKeyword::Afterlife, 0);
    let spirits = timestamps(&resolve(&dead));
    verify_tokens(&spirits, TokenKeyword::Afterlife, 2);

    let cast = advance(
        &game,
        A,
        GameCommand::CastUntargeted {
            spell: object(&game, 2),
            payment: Some(ManaPayment {
                from_lots: Default::default(),
                spend: vec![ManaSpend {
                    mana_type: ManaType::White,
                    amount: 4.into(),
                }],
            }),
        },
    );
    let entered = resolve(&cast);
    assert_eq!(entered.execution().window().entries().len(), 1);
    assert!(
        entered.execution().window().entries()[0]
            .targets()
            .targets()
            .is_empty()
    );
    let offered = resolve(&entered);
    let RuleFrame::OptionalEffect(frame) = offered.pending_decision().unwrap().frame() else {
        panic!("fabricate offers its controller a choice")
    };
    assert_eq!(
        frame.options(),
        &BTreeSet::from([OptionalEffectChoice::Take, OptionalEffectChoice::Decline])
    );
    let took = choose(
        &offered,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(
        took.execution()
            .power_toughness_counters(object(&took, 2))
            .unwrap()
            .plus_one,
        2.into()
    );
    verify_tokens(&took, TokenKeyword::Fabricate, 0);
    let declined = choose(
        &offered,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    verify_tokens(&timestamps(&declined), TokenKeyword::Fabricate, 2);
    assert_eq!(
        declined
            .execution()
            .power_toughness_counters(object(&declined, 2))
            .unwrap()
            .plus_one,
        0.into()
    );

    let bounced = resolve(&cast_at(&entered, 4, 2));
    let absent = resolve(&bounced);
    let RuleFrame::OptionalEffect(frame) = absent.pending_decision().unwrap().frame() else {
        panic!("expected fallback")
    };
    assert_eq!(
        frame.options(),
        &BTreeSet::from([OptionalEffectChoice::Decline])
    );
    let pending = absent.pending_decision().unwrap();
    assert!(
        absent
            .advance(GameInput {
                basis: absent.basis().clone(),
                actor: A,
                command: GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::OptionalEffect(OptionalEffectChoice::Take)
                }
            })
            .is_err()
    );
    let guaranteed = choose(
        &absent,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    verify_tokens(&timestamps(&guaranteed), TokenKeyword::Fabricate, 2);
}

#[test]
fn fabricate_and_afterlife_support_noncreature_sources_graveyards_and_controller_choices() {
    let v = Arc::new(
        CreatureTypeVocabulary::new(
            "cr-20260925".into(),
            [0; 32],
            vec!["Servo".into(), "Spirit".into()],
        )
        .unwrap(),
    );
    let repeated = parse_keywords("Afterlife 2\nFabricate 1, Afterlife 1", Some(&v), None).unwrap();
    assert_eq!(repeated.triggers.len(), 3);
    for (index, keyword, amount) in [
        (0, TokenKeyword::Afterlife, 2),
        (1, TokenKeyword::Fabricate, 1),
        (2, TokenKeyword::Afterlife, 1),
    ] {
        assert_eq!(
            repeated.triggers[index],
            keyword
                .ability(TriggeredAbilityId(index as u64), amount.into(), &v)
                .unwrap()
        );
    }
    for row in ["Fabricate 01", "Afterlife -1", "Fabricate 1\nDraw a card."] {
        assert!(parse_keywords(row, Some(&v), None).is_none());
    }
    let mut afterlife = Program::artifact(None, vec![]);
    afterlife.triggered.push(
        TokenKeyword::Afterlife
            .ability(TriggeredAbilityId(0), 2.into(), &v)
            .unwrap(),
    );
    let mut fabricate =
        Program::creature(fixed_cost("{2}{W}{W}"), 2.into(), 2.into(), BTreeSet::new());
    fabricate.triggered.push(
        TokenKeyword::Fabricate
            .ability(TriggeredAbilityId(0), 2.into(), &v)
            .unwrap(),
    );
    for (keyword, p) in [
        (TokenKeyword::Afterlife, &afterlife),
        (TokenKeyword::Fabricate, &fabricate),
    ] {
        assert_eq!(
            TokenKeyword::from_ability(&p.triggered[0], &v),
            Some(keyword)
        );
        assert!(
            keyword
                .ability(TriggeredAbilityId(0), (-1).into(), &v)
                .is_err()
        );
    }
    scenarios(v, afterlife, fabricate);
}

#[test]
#[ignore = "requires the pinned Oracle snapshot and creature vocabulary"]
fn debtors_transport_and_visionary_augmenter_create_their_source_specified_tokens() {
    let v = vocabulary::load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap();
    let catalogue = spellwake_content::snapshot::load(
        &input("SPELLWAKE_CATALOGUE_METADATA"),
        &input("SPELLWAKE_CATALOGUE_INPUT"),
    )
    .unwrap();
    let registry = Registry::from_keyword_gameplay_inventory(&catalogue, &v).unwrap();
    let source = |name| {
        let row = registry
            .coverage
            .cards
            .iter()
            .find(|c| c.name == name)
            .unwrap();
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        registry.bindings[&id].instruction.clone()
    };
    scenarios(
        v,
        source("Debtors' Transport"),
        source("Visionary Augmenter"),
    );
}
