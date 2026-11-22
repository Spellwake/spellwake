//! Resolve each admitted real card through a discard and its madness trigger.
use super::*;
use spellwake_engine::{
    effects::ExileCastAbility,
    game::{DecisionReply, RuleFrame},
    mana::CastingPayment,
    program::{AffectedPlayers, BodyTarget, ExecutableBody, ProgramKind, ResolvingPlayer},
    targets::TargetRef,
};
fn obj(g: &GameState, n: u64) -> spellwake_engine::object_identity::ObjectId {
    g.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn zone(g: &GameState, n: u64) -> ObjectZone {
    g.execution()
        .window()
        .cards()
        .get(obj(g, n))
        .unwrap()
        .zone()
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn all_real_madness_cards_discard_then_pay_the_printed_price_and_resolve() {
    let (inventory, vocabulary, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let mut tested = BTreeSet::new();
    for row in inventory
        .cards
        .iter()
        .filter(|r| r.required_by_base_formats)
    {
        let id = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        let Some(bound) = registry.resolve(&id) else {
            continue;
        };
        let Some(cost) = bound.instruction.madness.as_ref() else {
            continue;
        };
        let text = without_reminder(row.faces[0].oracle_text.as_deref().unwrap()).unwrap();
        let raw = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("Madness "))
            .expect("printed madness");
        assert_eq!(Some(cost.clone()), fixed_cost(raw));
        let vanilla =
            Program::creature(Some(ManaCost::free()), 1.into(), 10.into(), BTreeSet::new());
        let mut cards = vec![
            registry
                .seed(&id, CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
        ];
        let mut add = |n, owner, destination, program| {
            cards.push(SeedCard {
                card: CardId(n),
                owner,
                destination,
                program,
            })
        };
        add(
            2,
            a,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::Discard {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
                    }],
                ),
            ),
        );
        add(3, a, ObjectDestination::Hand, vanilla.clone());
        add(
            4,
            b,
            ObjectDestination::Battlefield { controller: b },
            vanilla.clone(),
        );
        add(5, a, ObjectDestination::LibraryBottom, vanilla);
        let mut pool = cost.required.clone();
        if cost.generic.is_positive() {
            pool.insert(ManaType::Colourless, cost.generic.clone());
        }
        let mana = pool
            .iter()
            .map(|(&mana_type, amount)| FloatingMana {
                player: a,
                mana_type,
                amount: amount.clone(),
            })
            .collect();
        let payment = (!pool.is_empty()).then(|| ManaPayment {
            from_lots: Default::default(),
            spend: pool
                .into_iter()
                .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                .collect(),
        });
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "registry16".into(),
            "madness".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(4)],
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary.clone(),
            names.clone(),
        );
        let init = GameState::initialize_scheduled(
            GameId(16),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::PostcombatMain,
            },
        )
        .unwrap()
        .into_state();
        let stacked = advance(
            &init,
            a,
            GameCommand::CastUntargeted {
                spell: obj(&init, 3),
                payment: None,
            },
        );
        let original_entry = stacked.execution().window().entries().last().unwrap().id();
        let cast = advance(
            &stacked,
            a,
            GameCommand::CastUntargeted {
                spell: obj(&stacked, 2),
                payment: None,
            },
        );
        let pending = exile_hand::until(cast, |s| {
            s.pending_decision().is_some_and(|p|matches!(p.frame(), RuleFrame::OptionalEffect(frame) if frame.exile_cast().is_some_and(|offer|offer.ability==ExileCastAbility::Madness)))
        });
        let p = pending.pending_decision().unwrap();
        assert_eq!(zone(&pending, 1), ObjectZone::Exile);
        let target = match &bound.instruction.kind {
            ProgramKind::Spell(s) if matches!(s.body.target, BodyTarget::SingleSpell(_)) => {
                Some(TargetRef::Stack(original_entry))
            }
            ProgramKind::Spell(s) if s.body.target == BodyTarget::SingleCreature => {
                Some(TargetRef::Object(obj(&pending, 4)))
            }
            _ => None,
        };
        let paid = advance(
            &pending,
            a,
            GameCommand::Reply {
                decision: p.id(),
                choice: DecisionReply::MadnessCast {
                    target,
                    payment: CastingPayment {
                        mana: payment,
                        ..Default::default()
                    },
                    mana_abilities: vec![],
                },
            },
        );
        assert_eq!(zone(&paid, 1), ObjectZone::Stack);
        let done = exile_hand::until(paid, |s| zone(s, 1) != ObjectZone::Stack);
        if matches!(bound.instruction.kind, ProgramKind::Permanent(_)) {
            assert_eq!(zone(&done, 1), ObjectZone::Battlefield);
        } else {
            assert_eq!(zone(&done, 1), ObjectZone::Graveyard(a));
            match row.name.as_str() {
                "Ichor Slick" => {
                    let sheet = done
                        .execution()
                        .current_characteristics(obj(&done, 4))
                        .unwrap()
                        .unwrap();
                    let pt = sheet.power_toughness.unwrap().derived;
                    assert_eq!((pt.power, pt.toughness), ((-2).into(), 7.into()));
                    assert_eq!(bound.instruction.activated.len(), 1);
                }
                "Broken Concentration" => assert_eq!(zone(&done, 3), ObjectZone::Graveyard(a)),
                "Gisa's Bidding" => assert_eq!(
                    done.execution()
                        .window()
                        .cards()
                        .iter()
                        .filter(|c| c.is_token() && c.zone() == ObjectZone::Battlefield)
                        .count(),
                    2
                ),
                "Obsessive Search" => assert_eq!(zone(&done, 5), ObjectZone::Hand(a)),
                "Just the Wind" => assert_eq!(zone(&done, 4), ObjectZone::Hand(b)),
                "Terminal Agony" => assert_eq!(zone(&done, 4), ObjectZone::Graveyard(b)),
                "Alchemist's Greeting" => assert_eq!(
                    done.execution().battlefield_marked_damage(obj(&done, 4)),
                    Some(4.into())
                ),
                "Distemper of the Blood" => assert!(
                    done.execution()
                        .current_characteristics(obj(&done, 4))
                        .unwrap()
                        .unwrap()
                        .has_keyword(KeywordAbility::Trample)
                ),
                other => panic!("verify the printed spell body for {other}"),
            }
        }
        tested.insert(row.name.clone());
    }
    println!(
        "{} real madness cards: {}",
        tested.len(),
        tested.iter().cloned().collect::<Vec<_>>().join(", ")
    );
    assert!(tested.len() >= 14);
}
