//! Named games for instant-or-sorcery token watchers.
use super::*;
use spellwake_engine::{
    game::{DecisionReply, RuleFrame},
    object_identity::ObjectId,
    program::{BodyTarget, ExecutableBody, Instruction},
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn id(g: &GameState, n: u64) -> ObjectId {
    g.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn command(g: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    g.advance(GameInput {
        basis: g.basis().clone(),
        actor,
        command,
    })
    .unwrap()
    .into_state()
}
fn card(n: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(n),
        owner,
        destination,
        program,
    }
}
fn finish(mut state: GameState, watch: Option<u64>) -> (GameState, bool) {
    let mut while_on_stack = false;
    for _ in 0..40 {
        if let Some(p) = state.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Triggers(f) => DecisionReply::OrderTriggers(f.instances().to_vec()),
                f => panic!("unexpected {f:?}"),
            };
            state = command(
                &state,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice,
                },
            );
        } else if state.execution().window().entries().is_empty() {
            return (state, while_on_stack);
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                state.execution().window().phase()
            else {
                panic!("priority")
            };
            state = command(&state, holder, GameCommand::Pass);
        }
        if let Some(n) = watch {
            let zone = state
                .execution()
                .window()
                .cards()
                .get(id(&state, n))
                .unwrap()
                .zone();
            while_on_stack |= zone == ObjectZone::Stack
                && state
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .any(|o| o.is_token() && o.zone() == ObjectZone::Battlefield);
        }
    }
    panic!("stack failed to settle")
}
fn game(name: &str, active: SeatId, cast_source: bool) -> GameState {
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
        .clone()
        .unwrap();
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
    let body = || {
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::DrawController {
                amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
            }],
        )
    };
    let mut cards = vec![
        source,
        card(
            2,
            active,
            ObjectDestination::Hand,
            Program::instant(ManaCost::free(), body()),
        ),
        card(
            3,
            active,
            ObjectDestination::Hand,
            Program::sorcery(ManaCost::free(), body()),
        ),
        card(
            4,
            active,
            ObjectDestination::Hand,
            Program::artifact(Some(ManaCost::free()), vec![]),
        ),
    ];
    cards.push(card(
        5,
        active,
        ObjectDestination::Hand,
        Program::enchantment(Some(ManaCost::free())),
    ));
    let mut enchantment_creature =
        Program::creature(Some(ManaCost::free()), 2.into(), 2.into(), BTreeSet::new());
    enchantment_creature
        .card_types
        .insert(CardType::Enchantment);
    cards.push(card(
        6,
        active,
        ObjectDestination::Hand,
        enchantment_creature,
    ));
    if name == "Talrand, Sky Summoner" {
        cards.push(
            registry
                .seed(&oid, CardId(7), A, ObjectDestination::Hand)
                .unwrap(),
        );
    }
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
        "spell-token-watchers".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let initial = GameSeed::with_uniform_life(
        vec![A, B],
        active,
        cards,
        if cast_source { vec![] } else { vec![CardId(1)] },
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let mut state = GameState::initialize_scheduled(
        GameId(905),
        ctx,
        initial,
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
        let cast = command(
            &state,
            A,
            GameCommand::CastUntargeted {
                spell: id(&state, 1),
                payment: Some(payment),
            },
        );
        state = finish(cast, None).0;
    }
    let face = &row.faces[0];
    assert_eq!(
        cost,
        fixed_cost(face.mana_cost.as_deref().unwrap()).unwrap()
    );
    let sheet = state
        .execution()
        .current_characteristics(id(&state, 1))
        .unwrap()
        .unwrap();
    assert_eq!(
        sheet
            .current_names
            .iter()
            .map(|n| n.as_str())
            .collect::<Vec<_>>(),
        [name]
    );
    let type_line = face.type_line.as_deref().unwrap();
    let colors = cost
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
    assert_eq!(sheet.current_colors, colors, "{name}");
    if let Some((kind, subtypes)) = type_line.split_once(" — ") {
        assert_eq!(sheet.current_types, [CardType::Creature].into());
        assert_eq!(
            sheet.current_supertypes.contains(&Supertype::Legendary),
            kind == "Legendary Creature"
        );
        let vocabulary = current_catalogue().1;
        assert_eq!(
            sheet.current_creature_subtypes,
            subtypes
                .split_whitespace()
                .map(|s| vocabulary.resolve_label(s).unwrap())
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
fn tokens(state: &GameState) -> Vec<ObjectId> {
    state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .map(|o| o.id())
        .collect()
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_spell_token_watchers_create_exact_flying_tokens_on_controller_casts() {
    for (name, subtypes, size) in [
        ("Talrand, Sky Summoner", vec!["Drake"], 2),
        ("Murmuring Mystic", vec!["Bird", "Illusion"], 1),
    ] {
        let state = game(name, A, true);
        let artifact = command(
            &state,
            A,
            GameCommand::CastUntargeted {
                spell: id(&state, 4),
                payment: None,
            },
        );
        let state = finish(artifact, None).0;
        assert!(tokens(&state).is_empty(), "{name} artifact");
        let instant = command(
            &state,
            A,
            GameCommand::CastUntargeted {
                spell: id(&state, 2),
                payment: None,
            },
        );
        let (state, early) = finish(instant, Some(2));
        assert!(early, "{name} token before spell resolution");
        assert_eq!(tokens(&state).len(), 1);
        let sorcery = command(
            &state,
            A,
            GameCommand::CastUntargeted {
                spell: id(&state, 3),
                payment: None,
            },
        );
        let (state, _) = finish(sorcery, None);
        let produced = tokens(&state);
        assert_eq!(produced.len(), 2);
        for id in produced {
            let sheet = state
                .execution()
                .current_characteristics(id)
                .unwrap()
                .unwrap();
            assert_eq!(sheet.controller, Some(A));
            assert_eq!(sheet.current_types, [CardType::Creature].into());
            let expected_name = format!("{} Token", subtypes.join(" "));
            assert_eq!(
                sheet
                    .current_names
                    .iter()
                    .map(|n| n.as_str())
                    .collect::<Vec<_>>(),
                [expected_name.as_str()]
            );
            assert_eq!(sheet.current_colors, [Color::Blue].into());
            let v = current_catalogue().1;
            assert_eq!(
                sheet.current_creature_subtypes,
                subtypes
                    .iter()
                    .map(|s| v.resolve_label(s).unwrap())
                    .collect()
            );
            assert!(sheet.has_keyword(KeywordAbility::Flying));
            let pt = sheet.power_toughness.unwrap().derived;
            assert_eq!(pt.power, size.into());
            assert_eq!(pt.toughness, size.into());
        }
        let other = game(name, B, false);
        let spell = command(
            &other,
            B,
            GameCommand::CastUntargeted {
                spell: id(&other, 2),
                payment: None,
            },
        );
        assert!(
            tokens(&finish(spell, None).0).is_empty(),
            "{name} opponent cast"
        );
    }
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn spell_token_watcher_compiler_rejects_extra_or_altered_clauses() {
    let (inventory, v, _) = current_catalogue();
    let original = inventory
        .cards
        .iter()
        .find(|r| r.name == "Murmuring Mystic")
        .unwrap();
    for text in [
        "Whenever you cast an instant or sorcery spell, create a 1/1 blue Bird Illusion creature token with flying.\nFlying",
        "Whenever an opponent casts an instant or sorcery spell, create a 1/1 blue Bird Illusion creature token with flying.",
    ] {
        let mut card = original.clone();
        card.faces[0].oracle_text = Some(text.into());
        assert!(crate::keyword_creatures::spell_token_watchers::compile(&card, &v).is_none());
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn sigil_of_the_empty_throne_creates_angels_only_for_controllers_enchantment_casts() {
    let state = game("Sigil of the Empty Throne", A, true);
    let artifact = command(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: id(&state, 4),
            payment: None,
        },
    );
    let state = finish(artifact, None).0;
    let instant = command(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: id(&state, 2),
            payment: None,
        },
    );
    let state = finish(instant, None).0;
    assert!(tokens(&state).is_empty());
    let enchantment = command(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: id(&state, 5),
            payment: None,
        },
    );
    let (state, early) = finish(enchantment, Some(5));
    assert!(early);
    assert_eq!(tokens(&state).len(), 1);
    let creature = command(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: id(&state, 6),
            payment: None,
        },
    );
    let state = finish(creature, None).0;
    let produced = tokens(&state);
    assert_eq!(produced.len(), 2);
    for id in produced {
        let sheet = state
            .execution()
            .current_characteristics(id)
            .unwrap()
            .unwrap();
        assert_eq!(sheet.controller, Some(A));
        assert_eq!(sheet.current_types, [CardType::Creature].into());
        assert_eq!(sheet.current_colors, [Color::White].into());
        assert_eq!(
            sheet.current_creature_subtypes,
            [current_catalogue().1.resolve_label("Angel").unwrap()].into()
        );
        assert_eq!(
            sheet
                .current_names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>(),
            ["Angel Token"]
        );
        assert!(sheet.has_keyword(KeywordAbility::Flying));
        let stats = sheet.power_toughness.unwrap().derived;
        assert_eq!(stats.power, 4.into());
        assert_eq!(stats.toughness, 4.into());
    }
    let opponent = game("Sigil of the Empty Throne", B, false);
    let cast = command(
        &opponent,
        B,
        GameCommand::CastUntargeted {
            spell: id(&opponent, 5),
            payment: None,
        },
    );
    assert!(tokens(&finish(cast, None).0).is_empty());
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn talrand_obeys_the_legend_rule() {
    use spellwake_engine::checks::CheckDecision;
    let state = game("Talrand, Sky Summoner", A, false);
    let payment = ManaPayment {
        from_lots: Default::default(),
        spend: vec![
            ManaSpend {
                mana_type: ManaType::Colourless,
                amount: 2.into(),
            },
            ManaSpend {
                mana_type: ManaType::Blue,
                amount: 2.into(),
            },
        ],
    };
    let cast = command(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: id(&state, 7),
            payment: Some(payment),
        },
    );
    let passed = command(&cast, A, GameCommand::Pass);
    let choice = command(&passed, B, GameCommand::Pass);
    let RuleFrame::Checks(CheckDecision::Legend(frame)) =
        choice.pending_decision().unwrap().frame()
    else {
        panic!("legend choice")
    };
    let conflict = frame.conflicts()[0].id();
    let result = command(
        &choice,
        A,
        GameCommand::Reply {
            decision: choice.pending_decision().unwrap().id(),
            choice: DecisionReply::KeepLegend {
                conflict,
                keep: id(&choice, 1),
            },
        },
    );
    assert_eq!(
        result
            .execution()
            .window()
            .cards()
            .get(id(&result, 7))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(
        result
            .execution()
            .window()
            .cards()
            .get(id(&result, 1))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
}
