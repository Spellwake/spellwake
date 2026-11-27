//! Games with actual registered permanents, stack decisions, and distinct outcomes.
use super::*;
use crate::keyword_creatures::tests::current_catalogue;
use spellwake_engine::{
    characteristics::Color,
    combat::CombatRules,
    game::{DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
    mana::{FloatingMana, ManaPayment, ManaSpend},
    object_identity::{ObjectId, ObjectZone},
    priority::PriorityPhase,
    program::ProgramKind,
    setup::GameSeed,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use std::sync::Arc;
fn object(g: &GameState, n: u64) -> ObjectId {
    g.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn command(g: &GameState, actor: SeatId, cmd: GameCommand) -> GameState {
    g.advance(GameInput {
        basis: g.basis().clone(),
        actor,
        command: cmd,
    })
    .unwrap()
    .into_state()
}
fn settled(mut g: GameState, subject: u64) -> GameState {
    for _ in 0..45 {
        if let Some(p) = g.pending_decision() {
            let answer = match p.frame() {
                RuleFrame::Triggers(f) => DecisionReply::OrderTriggers(f.instances().to_vec()),
                RuleFrame::TriggerTarget(_) => {
                    DecisionReply::TriggerTarget(TargetRef::Object(object(&g, subject)))
                }
                RuleFrame::ResolutionGraveyardOrder(f) => {
                    DecisionReply::OrderResolutionGraveyard(f.domain().to_vec())
                }
                RuleFrame::TimestampOrder(f) => {
                    DecisionReply::TimestampOrder(f.members().iter().map(|m| m.member).collect())
                }
                f => panic!("unexpected decision {f:?}"),
            };
            g = command(
                &g,
                p.chooser(),
                GameCommand::Reply {
                    decision: p.id(),
                    choice: answer,
                },
            );
        } else if g.execution().window().entries().is_empty() {
            return g;
        } else {
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                g.execution().window().phase()
            else {
                panic!("priority")
            };
            g = command(&g, holder, GameCommand::Pass);
        }
    }
    panic!("trigger did not finish")
}
#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn named_ordinary_permanents_settle_their_full_entry_and_death_triggers() {
    let (inventory, vocab, registry) = current_catalogue();
    let (a, b) = (SeatId(0), SeatId(1));
    for name in [
        "Cathedral Sanctifier",
        "Cloudkin Seer",
        "Feral Prowler",
        "Ironshell Beetle",
        "Attended Knight",
        "Wailing Ghoul",
    ] {
        let card = inventory.cards.iter().find(|r| r.name == name).unwrap();
        let id = OracleId::try_from(card.oracle_id.clone().unwrap()).unwrap();
        assert!(
            compile(card, &vocab).is_some(),
            "ordinary triggered source {name}"
        );
        let bound = registry.resolve(&id).unwrap();
        let ProgramKind::Permanent(def) = &bound.instruction.kind else {
            panic!("permanent")
        };
        let printed = fixed_cost(card.faces[0].mana_cost.as_deref().unwrap()).unwrap();
        assert_eq!(def.casting_cost.as_ref(), Some(&printed));
        let mut cost = printed.required;
        if printed.generic.is_positive() {
            cost.insert(ManaType::Colourless, printed.generic);
        }
        let mana = cost
            .iter()
            .map(|(&mana_type, amount)| FloatingMana {
                player: a,
                mana_type,
                amount: amount.clone(),
            })
            .collect();
        let pay = ManaPayment {
            from_lots: Default::default(),
            spend: cost
                .into_iter()
                .map(|(mana_type, amount)| ManaSpend { mana_type, amount })
                .collect(),
        };
        let vanilla = Program::creature(None, 1.into(), 5.into(), BTreeSet::new());
        let mut cards = vec![
            registry
                .seed(&id, CardId(1), a, ObjectDestination::Hand)
                .unwrap(),
        ];
        cards.push(SeedCard {
            card: CardId(2),
            owner: a,
            destination: ObjectDestination::Battlefield { controller: a },
            program: vanilla.clone(),
        });
        cards.push(SeedCard {
            card: CardId(3),
            owner: a,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SINGLE_CREATURE,
                    vec![
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::zone_op(
                            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                                spellwake_engine::toolbox::ObjectQuery::Targets,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                                            allow_regenerate: true,
                                        },
                                },
                            ),
                        ),
                    ],
                ),
            ),
        });
        for n in 10..14 {
            cards.push(SeedCard {
                card: CardId(n),
                owner: a,
                destination: ObjectDestination::LibraryBottom,
                program: vanilla.clone(),
            });
        }
        let rules = Arc::new(spellwake_contracts::names::NameRules::literal_names());
        let ctx = GameContext::new(
            CURRENT_KEYWORD_PROFILE.into(),
            vocab.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            "current".into(),
            "ordinary-trigger".into(),
            vocab.id(),
            rules.id(),
        )
        .unwrap();
        let seed = GameSeed::with_uniform_life(
            vec![a, b],
            a,
            cards,
            vec![CardId(2)],
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab.clone(),
            rules,
        );
        let mut g = GameState::initialize_scheduled(
            GameId(402),
            ctx,
            seed,
            TurnStart {
                number: 1,
                step: Step::PostcombatMain,
            },
        )
        .unwrap()
        .into_state();
        g = command(
            &g,
            a,
            GameCommand::CastUntargeted {
                spell: object(&g, 1),
                payment: Some(pay),
            },
        );
        g = settled(g, 2);
        assert_eq!(
            g.execution()
                .window()
                .cards()
                .get(object(&g, 1))
                .unwrap()
                .zone(),
            ObjectZone::Battlefield
        );
        match name {
            "Cathedral Sanctifier" => assert_eq!(g.execution().life_total(a), Some(&23.into())),
            "Cloudkin Seer" => assert_eq!(
                g.execution()
                    .window()
                    .cards()
                    .get(object(&g, 10))
                    .unwrap()
                    .zone(),
                ObjectZone::Hand(a)
            ),
            "Feral Prowler" => {
                assert_eq!(g.execution().window().cards().library(a).len(), 4);
                g = command(
                    &g,
                    a,
                    GameCommand::Cast {
                        spell: object(&g, 3),
                        target: TargetRef::Object(object(&g, 1)),
                    },
                );
                g = settled(g, 2);
                assert_eq!(
                    g.execution()
                        .window()
                        .cards()
                        .get(object(&g, 1))
                        .unwrap()
                        .zone(),
                    ObjectZone::Graveyard(a)
                );
                assert_eq!(
                    g.execution()
                        .window()
                        .cards()
                        .get(object(&g, 10))
                        .unwrap()
                        .zone(),
                    ObjectZone::Hand(a)
                );
            }
            "Ironshell Beetle" => assert_eq!(
                g.execution()
                    .power_toughness_counters(object(&g, 2))
                    .unwrap()
                    .plus_one,
                1.into()
            ),
            "Attended Knight" => {
                let tokens: Vec<_> = g
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
                    .collect();
                assert_eq!(tokens.len(), 1);
                let t = g
                    .execution()
                    .current_characteristics(tokens[0].id())
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    t.current_creature_subtypes,
                    [vocab.resolve_label("Soldier").unwrap()].into()
                );
                assert_eq!(t.current_colors, [Color::White].into());
                let pt = t.power_toughness.unwrap().derived;
                assert_eq!((pt.power, pt.toughness), (1.into(), 1.into()));
            }
            "Wailing Ghoul" => assert_eq!(g.execution().window().cards().library(a).len(), 2),
            _ => unreachable!(),
        }
    }
}
