//! Each admitted Oracle row is cast, copied on the stack, and resolved twice.
use super::*;
use spellwake_engine::{
    effects::{CopyTargetChoice, LibraryTopReply},
    game::{DecisionReply, RuleFrame},
    mana::CastingPayment,
    object_identity::ObjectId,
    program::{BodyTarget, CastCopyKeyword, ExecutableBody, Instruction, ProgramKind},
    stack_window::StackKind,
    targets::TargetRef,
};
fn obj(g: &GameState, n: u64) -> ObjectId {
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
fn proceed(g: &GameState) -> GameState {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = g.execution().window().phase()
    else {
        panic!("expected priority")
    };
    advance(g, holder, GameCommand::Pass)
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn all_real_cast_copy_spells_resolve_original_and_copy() {
    let (inventory, vocab, registry) = current_catalogue();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let (a, b) = (SeatId(0), SeatId(1));
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
        let Some(copy) = bound
            .instruction
            .triggered
            .iter()
            .find_map(|t| t.cast_copy_keyword())
        else {
            continue;
        };
        let label = match copy {
            CastCopyKeyword::CopyThisPermanentSpell
            | CastCopyKeyword::CopyObservedSpell
            | CastCopyKeyword::OfferCopyToOtherPlayers => continue,
            CastCopyKeyword::Storm => "Storm",
            CastCopyKeyword::Gravestorm => "Gravestorm",
            CastCopyKeyword::Replicate(_) => "Replicate ",
        };
        let ProgramKind::Spell(spell) = &bound.instruction.kind else {
            panic!("instruction spell")
        };
        let printed = without_reminder(row.faces[0].oracle_text.as_deref().unwrap()).unwrap();
        assert!(printed.lines().any(|l| l.trim().starts_with(label)));
        let mut price = fixed_cost(row.faces[0].mana_cost.as_deref().unwrap()).unwrap();
        assert_eq!(price, spell.cost);
        let mut payment = CastingPayment::default();
        if let CastCopyKeyword::Replicate(index) = copy {
            let cost = fixed_cost(
                printed
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("Replicate "))
                    .unwrap(),
            )
            .unwrap();
            price.generic = price.generic.plus(&cost.generic);
            for (ty, n) in cost.required {
                let old = price.required.remove(&ty).unwrap_or_else(|| 0.into());
                price.required.insert(ty, old.plus(&n));
            }
            payment.additional.insert(index, 1.into());
        }
        if price.generic.is_positive() {
            price.required.insert(ManaType::Colourless, price.generic);
        }
        let funds = price
            .required
            .iter()
            .map(|(&mana_type, amount)| FloatingMana {
                player: a,
                mana_type,
                amount: amount.clone(),
            })
            .collect();
        payment.mana = Some(ManaPayment {
            from_lots: Default::default(),
            spend: price
                .required
                .into_iter()
                .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                .collect(),
        });
        let mut cards = vec![
            registry
                .seed(&id, CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
        ];
        let harmless = Program::creature(None, 1.into(), 10.into(), BTreeSet::new());
        for n in 2..=4 {
            cards.push(SeedCard {
                card: CardId(n),
                owner: b,
                destination: ObjectDestination::Battlefield { controller: b },
                program: harmless.clone(),
            });
        }
        let removal = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(BodyTarget::SingleCreature, vec![Instruction::DestroyTarget]),
        );
        cards.push(SeedCard {
            card: CardId(5),
            owner: a,
            destination: ObjectDestination::Hand,
            program: removal,
        });
        // With exactly two cards, each post-exile shuffle needs zero entropy words.
        let library_size = if copy == CastCopyKeyword::Gravestorm {
            2
        } else {
            8
        };
        for n in 0..library_size {
            for (offset, owner) in [(10, a), (20, b)] {
                cards.push(SeedCard {
                    card: CardId(offset + n),
                    owner,
                    destination: ObjectDestination::LibraryBottom,
                    program: harmless.clone(),
                });
            }
        }
        let context = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "registry17".into(),
            "cast-copy".into(),
            vocab.id(),
            names.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2), CardId(3), CardId(4)],
            funds,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab.clone(),
            names.clone(),
        );
        let mut g = GameState::initialize_scheduled(
            GameId(41),
            context,
            seed,
            TurnStart {
                number: 1,
                step: Step::PostcombatMain,
            },
        )
        .unwrap()
        .into_state();
        g = advance(
            &g,
            a,
            GameCommand::CastWithResources {
                spell: obj(&g, 5),
                target: Some(TargetRef::Object(obj(&g, 2))),
                payment: CastingPayment::default(),
            },
        );
        g = proceed(&g);
        g = proceed(&g);
        assert_eq!(zone(&g, 2), ObjectZone::Graveyard(b));
        let target = match spell.body.target {
            BodyTarget::None => None,
            BodyTarget::SinglePlayer => Some(TargetRef::Player(b)),
            BodyTarget::SingleCreature | BodyTarget::SinglePermanent => {
                Some(TargetRef::Object(obj(&g, 3)))
            }
            other => panic!("new target {other:?}"),
        };
        g = advance(
            &g,
            a,
            GameCommand::CastWithResources {
                spell: obj(&g, 1),
                target,
                payment,
            },
        );
        let mut saw_copy = false;
        let mut choices = 0;
        let mut searches = 0;
        for _ in 0..40 {
            if zone(&g, 1) != ObjectZone::Stack {
                break;
            }
            saw_copy |= g
                .execution()
                .window()
                .entries()
                .iter()
                .any(|e| matches!(e.kind(), StackKind::SpellCopy { .. }));
            if let Some(p) = g.pending_decision() {
                let reply = match p.frame() {
                    RuleFrame::Triggers(f) => DecisionReply::OrderTriggers(f.instances().to_vec()),
                    RuleFrame::CopyTargets(_) => {
                        choices += 1;
                        DecisionReply::CopyTargets(match spell.body.target {
                            BodyTarget::SingleCreature | BodyTarget::SinglePermanent => {
                                CopyTargetChoice::Change(TargetRef::Object(obj(&g, 4)))
                            }
                            _ => CopyTargetChoice::Retain,
                        })
                    }
                    RuleFrame::TimestampOrder(f) => DecisionReply::TimestampOrder(
                        f.members().iter().map(|m| m.member).collect(),
                    ),
                    RuleFrame::LibraryTop(f) => {
                        assert_eq!(f.search_exile_owner(), Some(b));
                        assert_eq!(p.chooser(), a);
                        assert!(f.requires_search_result());
                        searches += 1;
                        DecisionReply::LibraryTop(LibraryTopReply::Search {
                            selected: f.eligible_results().first().copied(),
                        })
                    }
                    RuleFrame::ResolutionGraveyardOrder(order) => {
                        DecisionReply::OrderResolutionGraveyard(order.domain().to_vec())
                    }
                    other => panic!("unexpected {other:?} in {}", row.name),
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
                g = proceed(&g);
            }
        }
        assert_eq!(zone(&g, 1), ObjectZone::Graveyard(a), "{}", row.name);
        assert!(saw_copy, "{}", row.name);
        assert_eq!(choices, usize::from(target.is_some()));
        let life = |p| g.execution().life_total(p).unwrap().clone();
        match row.name.as_str() {
            "Weather the Storm" => assert_eq!(life(a), 26.into()),
            "Tendrils of Agony" => assert_eq!((life(a), life(b)), (24.into(), 16.into())),
            "Bitter Ordeal" => {
                assert_eq!(searches, 2);
                assert_eq!(
                    (zone(&g, 20), zone(&g, 21)),
                    (ObjectZone::Exile, ObjectZone::Exile)
                );
            }
            "Brain Freeze" => assert_eq!(g.execution().window().cards().library(b).len(), 2),
            "Train of Thought" => assert_eq!(g.execution().window().cards().library(a).len(), 6),
            "Temporal Fissure" | "Vacuumelt" => assert_eq!(
                (zone(&g, 3), zone(&g, 4)),
                (ObjectZone::Hand(b), ObjectZone::Hand(b))
            ),
            "Scattershot" => {
                for n in [3, 4] {
                    assert_eq!(
                        g.execution().battlefield_marked_damage(obj(&g, n)),
                        Some(1.into())
                    );
                }
            }
            "Astral Steel" => {
                for n in [3, 4] {
                    let pt = g
                        .execution()
                        .current_characteristics(obj(&g, n))
                        .unwrap()
                        .unwrap()
                        .power_toughness
                        .unwrap()
                        .derived;
                    assert_eq!((pt.power, pt.toughness), (2.into(), 12.into()));
                }
            }
            "Empty the Warrens" | "Chatterstorm" | "Hunting Pack" => {
                let count = g
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|c| c.is_token() && c.zone() == ObjectZone::Battlefield)
                    .count();
                assert_eq!(
                    count,
                    if row.name == "Empty the Warrens" {
                        4
                    } else {
                        2
                    }
                );
            }
            other => panic!("new source card needs an independent outcome: {other}"),
        }
        tested.insert(row.name.to_owned());
    }
    let expected = [
        "Astral Steel",
        "Bitter Ordeal",
        "Brain Freeze",
        "Chatterstorm",
        "Empty the Warrens",
        "Hunting Pack",
        "Scattershot",
        "Temporal Fissure",
        "Tendrils of Agony",
        "Train of Thought",
        "Vacuumelt",
        "Weather the Storm",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(tested, expected);
    println!(
        "{} real cast-copy spells: {}",
        tested.len(),
        tested.into_iter().collect::<Vec<_>>().join(", ")
    );
}
