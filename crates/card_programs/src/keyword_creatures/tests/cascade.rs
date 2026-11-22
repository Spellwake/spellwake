//! Each admitted printed Cascade instance gets a real optional cast above its source.
use super::*;
use spellwake_engine::{
    effects::ExileCastAbility,
    game::{DecisionReply, RuleFrame},
    mana::CastingPayment,
    program::ProgramKind,
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
fn all_real_cascade_cards_resolve_each_printed_cast_trigger_before_the_original_spell() {
    let (inventory, vocab, registry) = current_catalogue();
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
        let cascade = bound
            .instruction
            .triggered
            .iter()
            .filter(|t| **t == TriggeredAbility::cascade(t.id))
            .count();
        if cascade == 0 {
            continue;
        }
        let raw = without_reminder(row.faces[0].oracle_text.as_deref().unwrap()).unwrap();
        let printed = raw
            .split([',', '\n'])
            .filter(|s| s.trim().eq_ignore_ascii_case("cascade"))
            .count();
        assert_eq!(cascade, printed);
        let cost = fixed_cost(row.faces[0].mana_cost.as_deref().unwrap()).unwrap();
        let mut pool = cost.required;
        if cost.generic.is_positive() {
            pool.insert(ManaType::Colourless, cost.generic);
        }
        let funded = pool
            .iter()
            .map(|(&mana_type, amount)| FloatingMana {
                player: a,
                mana_type,
                amount: amount.clone(),
            })
            .collect();
        let pay = ManaPayment {
            from_lots: Default::default(),
            spend: pool
                .into_iter()
                .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                .collect(),
        };
        let mut cards = vec![
            registry
                .seed(&id, CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
        ];
        let harmless =
            Program::creature(Some(ManaCost::free()), 1.into(), 10.into(), BTreeSet::new());
        let mut add = |n, owner, destination| {
            cards.push(SeedCard {
                card: CardId(n),
                owner,
                destination,
                program: harmless.clone(),
            })
        };
        add(2, b, ObjectDestination::Battlefield { controller: b });
        add(3, a, ObjectDestination::Hand);
        for n in 0..cascade as u64 {
            add(10 + n, a, ObjectDestination::LibraryBottom);
        }
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "registry16".into(),
            "cascade".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2)],
            funded,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab.clone(),
            names.clone(),
        );
        let mut g = GameState::initialize_scheduled(
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
        let target = match &bound.instruction.kind {
            ProgramKind::Spell(spell) => match spell.body.target {
                BodyTarget::SingleSpell(_) => {
                    g = advance(
                        &g,
                        a,
                        GameCommand::CastUntargeted {
                            spell: obj(&g, 3),
                            payment: None,
                        },
                    );
                    Some(TargetRef::Stack(
                        g.execution().window().entries().last().unwrap().id(),
                    ))
                }
                BodyTarget::SinglePermanent | BodyTarget::SingleCreature => {
                    Some(TargetRef::Object(obj(&g, 2)))
                }
                BodyTarget::None => None,
                _ => panic!("verify source target for {}", row.name),
            },
            ProgramKind::Permanent(_) => None,
        };
        let command = target.map_or_else(
            || GameCommand::CastUntargeted {
                spell: obj(&g, 1),
                payment: Some(pay.clone()),
            },
            |target| GameCommand::CastPaid {
                spell: obj(&g, 1),
                target,
                payment: pay.clone(),
            },
        );
        g = advance(&g, a, command);
        let mut hits = 0;
        for _ in 0..80 {
            if zone(&g, 1) != ObjectZone::Stack {
                break;
            }
            if let Some(p) = g.pending_decision() {
                let reply = match p.frame() {
                    RuleFrame::Triggers(frame) => {
                        DecisionReply::OrderTriggers(frame.instances().to_vec())
                    }
                    RuleFrame::OptionalEffect(frame)
                        if frame
                            .exile_cast()
                            .is_some_and(|offer| offer.ability == ExileCastAbility::Cascade) =>
                    {
                        assert_eq!(frame.exile_cast().unwrap().spell, obj(&g, 10 + hits));
                        hits += 1;
                        DecisionReply::CascadeCast {
                            target: None,
                            payment: CastingPayment::default(),
                        }
                    }
                    other => panic!("unexpected {other:?} for {}", row.name),
                };
                g = advance(
                    &g,
                    p.chooser(),
                    GameCommand::Reply {
                        decision: p.id(),
                        choice: reply,
                    },
                );
            } else {
                let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                    g.execution().window().phase()
                else {
                    panic!("expected priority");
                };
                g = advance(&g, holder, GameCommand::Pass);
            }
        }
        assert_eq!(hits as usize, cascade);
        assert_ne!(zone(&g, 1), ObjectZone::Stack);
        for n in 0..hits {
            assert_eq!(zone(&g, 10 + n), ObjectZone::Battlefield);
        }
        tested.insert(row.name.clone());
    }
    println!(
        "{} real cascade cards: {}",
        tested.len(),
        tested.iter().cloned().collect::<Vec<_>>().join(", ")
    );
    for name in [
        "Apex Devastator",
        "Bloodbraid Elf",
        "Boarding Party",
        "Shardless Agent",
        "Captured Sunlight",
        "Deny Reality",
        "Bituminous Blast",
        "Throes of Chaos",
        "Into the Time Vortex",
        "Forceful Denial",
    ] {
        assert!(tested.contains(name), "{name}");
    }
}
