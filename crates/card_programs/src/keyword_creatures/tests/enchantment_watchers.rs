//! Named source-qualified enchantment cast watcher games.
use super::*;
use spellwake_engine::{
    effects::OptionalEffectChoice,
    game::{DecisionReply, RuleFrame},
    object_identity::ObjectId,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn object(g: &GameState, id: u64) -> ObjectId {
    g.execution()
        .window()
        .cards()
        .current_identity(CardId(id))
        .unwrap()
}
fn apply(g: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    g.advance(GameInput {
        basis: g.basis().clone(),
        actor,
        command,
    })
    .unwrap()
    .into_state()
}
fn card(id: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}
fn library(g: &GameState, owner: SeatId) -> usize {
    g.execution().window().cards().library(owner).len()
}
fn finish(
    mut game: GameState,
    optional: OptionalEffectChoice,
    watch: Option<u64>,
) -> (GameState, usize, bool) {
    let before = library(&game, A);
    let mut offers = 0;
    let mut while_on_stack = false;
    for _ in 0..40 {
        if let Some(p) = game.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Triggers(f) => DecisionReply::OrderTriggers(f.instances().to_vec()),
                RuleFrame::OptionalEffect(_) => {
                    offers += 1;
                    DecisionReply::OptionalEffect(optional)
                }
                frame => panic!("unexpected {frame:?}"),
            };
            game = apply(
                &game,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice,
                },
            );
        } else if game.execution().window().entries().is_empty() {
            return (game, offers, while_on_stack);
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                game.execution().window().phase()
            else {
                panic!("priority")
            };
            game = apply(&game, holder, GameCommand::Pass);
        }
        if library(&game, A) < before
            && let Some(id) = watch
        {
            let zone = game
                .execution()
                .window()
                .cards()
                .get(object(&game, id))
                .unwrap()
                .zone();
            while_on_stack |= zone == ObjectZone::Stack;
        }
    }
    panic!("stack failed to settle")
}
fn setup(name: &str, actor: SeatId, cast_source: bool) -> GameState {
    let (inventory, vocabulary, registry) = current_catalogue();
    let row = inventory.cards.iter().find(|r| r.name == name).unwrap();
    let oid = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
    let source = registry
        .seed(
            &oid,
            CardId(1),
            A,
            if cast_source {
                ObjectDestination::Hand
            } else {
                ObjectDestination::Battlefield { controller: A }
            },
        )
        .unwrap();
    let cost = source
        .program
        .permanent()
        .unwrap()
        .casting_cost
        .as_ref()
        .unwrap()
        .clone();
    let mut funding = cost.required.clone();
    if cost.generic.is_positive() {
        funding.insert(ManaType::Colourless, cost.generic.clone());
    }
    let mana = funding
        .iter()
        .map(|(&mana_type, amount)| FloatingMana {
            player: A,
            mana_type,
            amount: amount.clone(),
        })
        .collect();
    let mut enchanted_creature =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    enchanted_creature.card_types.insert(CardType::Enchantment);
    let mut cards = vec![
        source,
        card(
            2,
            actor,
            ObjectDestination::Hand,
            Program::enchantment(Some(ManaCost::free())),
        ),
        card(
            3,
            actor,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
        card(4, actor, ObjectDestination::Hand, enchanted_creature),
    ];
    for player in [A, B] {
        for n in 0..5 {
            cards.push(card(
                100 + u64::from(player.0) * 10 + n,
                player,
                ObjectDestination::LibraryBottom,
                Program::artifact(None, vec![]),
            ));
        }
    }
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let ctx = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        format!(
            "keyword-gameplay-registry{}",
            registry.coverage.registry_version
        ),
        "enchantment-cast-watchers".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        actor,
        cards,
        if cast_source { vec![] } else { vec![CardId(1)] },
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let mut state = GameState::initialize_scheduled(
        GameId(903),
        ctx,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    if cast_source {
        let payment = ManaPayment {
            from_lots: Default::default(),
            spend: funding
                .into_iter()
                .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                .collect(),
        };
        state = apply(
            &state,
            A,
            GameCommand::CastUntargeted {
                spell: object(&state, 1),
                payment: Some(payment),
            },
        );
        let result = finish(state, OptionalEffectChoice::Take, None);
        assert_eq!(result.1, 0);
        state = result.0;
    }
    let face = &row.faces[0];
    assert_eq!(
        cost,
        fixed_cost(face.mana_cost.as_deref().unwrap()).unwrap(),
        "{name}"
    );
    let sheet = state
        .execution()
        .current_characteristics(object(&state, 1))
        .unwrap()
        .unwrap();
    assert_eq!(
        sheet
            .current_names
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>(),
        [name]
    );
    let expected_colors = cost
        .required
        .keys()
        .filter_map(|mana| match mana {
            ManaType::White => Some(Color::White),
            ManaType::Blue => Some(Color::Blue),
            ManaType::Black => Some(Color::Black),
            ManaType::Red => Some(Color::Red),
            ManaType::Green => Some(Color::Green),
            ManaType::Colourless => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(sheet.current_colors, expected_colors, "{name}");
    let type_line = face.type_line.as_deref().unwrap();
    if let Some((types, subtypes)) = type_line.split_once(" — ") {
        let mut expected = BTreeSet::from([CardType::Creature]);
        if types.split_whitespace().any(|t| t == "Enchantment") {
            expected.insert(CardType::Enchantment);
        }
        assert_eq!(sheet.current_types, expected);
        assert_eq!(
            sheet.current_supertypes.contains(&Supertype::Legendary),
            types.starts_with("Legendary ")
        );
        let vocabulary = current_catalogue().1;
        assert_eq!(
            sheet.current_creature_subtypes,
            subtypes
                .split_whitespace()
                .map(|label| vocabulary.resolve_label(label).unwrap())
                .collect()
        );
        let stats = sheet.power_toughness.unwrap().derived;
        assert_eq!(stats.power, number(face.power.as_deref()).unwrap().into());
        assert_eq!(
            stats.toughness,
            number(face.toughness.as_deref()).unwrap().into()
        );
    } else {
        assert_eq!(sheet.current_types, [CardType::Enchantment].into());
        assert!(sheet.power_toughness.is_none());
    }
    state
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_enchantress_watchers_draw_on_controller_cast_with_printed_optionality() {
    for (name, optional) in [
        ("Mesa Enchantress", true),
        ("Verduran Enchantress", true),
        ("Enchantress's Presence", false),
        ("Satyr Enchanter", false),
        ("Sythis, Harvest's Hand", false),
    ] {
        for accept in if optional {
            vec![true, false]
        } else {
            vec![true]
        } {
            let state = setup(name, A, true);
            let before = library(&state, A);
            let artifact = apply(
                &state,
                A,
                GameCommand::CastUntargeted {
                    spell: object(&state, 3),
                    payment: None,
                },
            );
            let (state, offers, _) = finish(artifact, OptionalEffectChoice::Take, None);
            assert_eq!(offers, 0, "{name} artifact");
            assert_eq!(library(&state, A), before);
            let enchantment = apply(
                &state,
                A,
                GameCommand::CastUntargeted {
                    spell: object(&state, 2),
                    payment: None,
                },
            );
            let choice = if accept {
                OptionalEffectChoice::Take
            } else {
                OptionalEffectChoice::Decline
            };
            let (state, offers, on_stack) = finish(enchantment, choice, Some(2));
            assert_eq!(offers, usize::from(optional), "{name}");
            assert_eq!(library(&state, A), before - usize::from(accept), "{name}");
            assert_eq!(on_stack, accept, "{name} draw before spell resolves");
            let life_gain = if name == "Sythis, Harvest's Hand" {
                1_i64
            } else {
                0_i64
            };
            assert_eq!(
                state.execution().life_total(A),
                Some(&(20 + life_gain).into()),
                "{name}"
            );
            let creature = apply(
                &state,
                A,
                GameCommand::CastUntargeted {
                    spell: object(&state, 4),
                    payment: None,
                },
            );
            let (state, offers, on_stack) = finish(creature, choice, Some(4));
            assert_eq!(offers, usize::from(optional));
            assert_eq!(library(&state, A), before - 2 * usize::from(accept));
            assert_eq!(on_stack, accept, "{name} enchantment creature");
            assert_eq!(
                state.execution().life_total(A),
                Some(&(20 + 2 * life_gain).into()),
                "{name}"
            );
        }
        let state = setup(name, B, false);
        let before = library(&state, A);
        let opponent = apply(
            &state,
            B,
            GameCommand::CastUntargeted {
                spell: object(&state, 2),
                payment: None,
            },
        );
        let (state, offers, _) = finish(opponent, OptionalEffectChoice::Take, None);
        assert_eq!(offers, 0, "{name} opponent");
        assert_eq!(library(&state, A), before);
        assert_eq!(
            state.execution().life_total(A),
            Some(&20.into()),
            "{name} opponent"
        );
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn enchantress_compiler_rejects_additional_or_changed_rules() {
    let (inventory, vocabulary, _) = current_catalogue();
    let original = inventory
        .cards
        .iter()
        .find(|r| r.name == "Mesa Enchantress")
        .unwrap();
    for text in [
        "Whenever you cast an enchantment spell, you may draw a card.\nFlying",
        "Whenever an opponent casts an enchantment spell, you may draw a card.",
        "Whenever you cast an enchantment spell, you may draw two cards.",
    ] {
        let mut card = original.clone();
        card.faces[0].oracle_text = Some(text.into());
        assert!(
            crate::keyword_creatures::enchantment_watchers::compile(&card, &vocabulary).is_none(),
            "{text}"
        );
    }
}
