use super::*;
use spellwake_engine::{
    actions::TimingError,
    characteristics::{
        ControlRecipient, KeywordAbility, PowerToughness, ResolvedDuration, ResolvedPartSpec,
        ResolvedPartsSpec,
    },
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision},
    damage::{DamageRecipient, DamageSourceId},
    effects::{DecisionReply, EffectError, ResolutionSelectionKind},
    game::{GameError, NativeCandidate, RuleFrame},
    mana::{CastResource, CastingPayment, ManaCost},
    object_identity::{GameObject, ObjectId},
    occurrence::{EventCause, RulesEvent, RulesOccurrence, RulesRecord, ZoneReason},
    program::{
        AdditionalCastCost, AffectedPlayers, BodyTarget, ConditionalCastCost, ExecutableBody,
        Instruction, PermanentTypeFilter, ProgramKind, PublicZoneDestination, ZoneActionKind,
        ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementSubject,
    },
    stack_window::StackKind,
    targets::TargetRef,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const C: SeatId = SeatId(2);
const RATS: &str = "c53d6d3e-64e3-48b9-afab-3bde3164888f";
const EXECUTIONER: &str = "c3c45d50-9038-41df-bb2f-9bc40071845b";
const LARVA: &str = "72a39730-acc3-470c-960e-3a91c5eae4be";

fn source(oracle: &str, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    current_catalogue()
        .2
        .seed(
            &OracleId::try_from(oracle.to_owned()).unwrap(),
            CardId(1),
            owner,
            destination,
        )
        .unwrap()
}

fn seed(id: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}

fn game(
    seats: Vec<SeatId>,
    mut cards: Vec<SeedCard>,
    budget: &[(ManaType, i64)],
    step: Step,
) -> GameState {
    for &player in &seats {
        cards.push(seed(
            100 + u64::from(player.0),
            player,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let (_, vocabulary) = dated_sources();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "batch-definitions".into(),
        "public-commands".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let battlefield = cards
        .iter()
        .filter(|card| matches!(card.destination, ObjectDestination::Battlefield { .. }))
        .map(|card| card.card)
        .collect();
    let mana = budget
        .iter()
        .map(|&(mana_type, amount)| FloatingMana {
            player: A,
            mana_type,
            amount: amount.into(),
        })
        .collect();
    let seed = GameSeed::with_uniform_life(
        seats,
        A,
        cards,
        battlefield,
        mana,
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    GameState::initialize_scheduled(GameId(723), context, seed, TurnStart { number: 1, step })
        .unwrap()
        .into_state()
}

fn physical(game: &GameState, card: u64) -> &GameObject {
    let cards = game.execution().window().cards();
    cards
        .get(cards.current_identity(CardId(card)).unwrap())
        .unwrap()
}

fn command(game: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command,
    })
    .unwrap()
}

fn reply(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    let decision = game.pending_decision().unwrap();
    command(
        game,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
    )
}

fn pass(game: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = game.execution().window().phase()
    else {
        panic!("expected ready priority")
    };
    command(game, holder, GameCommand::Pass)
}

fn round(mut game: GameState) -> NativeCandidate {
    for _ in 1..game.execution().window().participants().len() {
        game = pass(&game).into_state();
    }
    pass(&game)
}

fn events(candidate: &NativeCandidate) -> impl Iterator<Item = &RulesEvent> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
}

fn tokens(candidate: &NativeCandidate, controller: SeatId, count: usize) -> Vec<ObjectId> {
    let batches = events(candidate)
        .map(|event| {
            event
                .occurrences
                .iter()
                .filter_map(|row| match &row.value {
                    RulesOccurrence::TokenCreated { creator, after } => {
                        assert_eq!(*creator, controller);
                        assert_eq!(after.object.owner(), controller);
                        assert_eq!(after.object.controller(), Some(controller));
                        assert_eq!(after.object.zone(), ObjectZone::Battlefield);
                        Some(after.object.id())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|batch| !batch.is_empty())
        .collect::<Vec<_>>();
    if count == 0 {
        assert!(batches.is_empty());
        return vec![];
    }
    let [batch] = batches.as_slice() else {
        panic!("one simultaneous creation event")
    };
    assert_eq!(batch.len(), count);
    let present = candidate
        .state()
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|o| o.is_token() && o.zone() == ObjectZone::Battlefield)
        .map(GameObject::id)
        .collect::<BTreeSet<_>>();
    assert_eq!(present, batch.iter().copied().collect());
    batch.clone()
}

fn assert_creature(
    game: &GameState,
    object: ObjectId,
    printed: (&str, &[&str], i64, i64),
    types: &[CardType],
    keywords: &[KeywordAbility],
) {
    let (name, subtypes, power, toughness) = printed;
    let (_, vocabulary, _) = current_catalogue();
    let sheet = game
        .execution()
        .current_characteristics(object)
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
    assert_eq!(sheet.current_types, types.iter().copied().collect());
    assert!(sheet.current_supertypes.is_empty());
    assert_eq!(sheet.current_colors, [Color::Black].into());
    assert_eq!(
        sheet.current_creature_subtypes,
        subtypes
            .iter()
            .map(|label| { vocabulary.resolve_label(label).unwrap() })
            .collect()
    );
    assert_eq!(
        sheet.power_toughness.unwrap().derived,
        PowerToughness {
            power: power.into(),
            toughness: toughness.into()
        }
    );
    assert_eq!(sheet.keywords, keywords.iter().copied().collect());
}

fn spend(costs: &[(ManaType, i64)]) -> ManaPayment {
    ManaPayment {
        from_lots: Default::default(),
        spend: costs
            .iter()
            .map(|&(mana_type, amount)| ManaSpend {
                mana_type,
                amount: amount.into(),
            })
            .collect(),
    }
}

fn cast_untargeted(game: &GameState, id: u64, costs: &[(ManaType, i64)]) -> NativeCandidate {
    command(
        game,
        A,
        GameCommand::CastUntargeted {
            spell: physical(game, id).id(),
            payment: (!costs.is_empty()).then(|| spend(costs)),
        },
    )
}

fn targeted(game: &GameState, actor: SeatId, id: u64, target: TargetRef) -> NativeCandidate {
    command(
        game,
        actor,
        GameCommand::Cast {
            spell: physical(game, id).id(),
            target,
        },
    )
}

fn free_instant(target: BodyTarget, instruction: Instruction) -> Program {
    Program::instant(
        ManaCost::free(),
        ExecutableBody::new(target, vec![instruction]),
    )
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn lab_rats_uses_printed_buyback_and_only_successful_resolution_creates_a_rat() {
    let rats = source(RATS, A, ObjectDestination::Hand);
    assert_eq!(rats.program.card_types, [CardType::Sorcery].into());
    let ProgramKind::Spell(spell) = &rats.program.kind else {
        panic!("sorcery")
    };
    assert_eq!(spell.cost, ManaCost::one(ManaType::Black));
    assert_eq!(spell.body.target, BodyTarget::None);
    assert_eq!(
        &rats.program.additional_cast_costs,
        &[AdditionalCastCost::Buyback(ManaCost::generic(4.into())),]
    );

    let early = game(
        vec![A, B],
        vec![rats.clone()],
        &[(ManaType::Black, 1), (ManaType::Colourless, 4)],
        Step::BeginningCombat,
    );
    assert!(matches!(
        early.advance(GameInput {
            basis: early.basis().clone(),
            actor: A,
            command: GameCommand::CastUntargeted {
                spell: physical(&early, 1).id(),
                payment: None,
            },
        }),
        Err(GameError::Effect(EffectError::Timing(
            TimingError::MainPhaseRequired
        )))
    ));

    for (buyback, counter, destination) in [
        (false, false, ObjectZone::Graveyard(A)),
        (true, false, ObjectZone::Hand(A)),
        (true, true, ObjectZone::Graveyard(A)),
    ] {
        let game = game(
            vec![A, B],
            vec![
                rats.clone(),
                seed(
                    2,
                    B,
                    ObjectDestination::Hand,
                    free_instant(
                        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                        Instruction::CounterSpell,
                    ),
                ),
            ],
            &[(ManaType::Black, 1), (ManaType::Colourless, 4)],
            Step::PrecombatMain,
        );
        let mut payment = vec![(ManaType::Black, 1)];
        if buyback {
            payment.push((ManaType::Colourless, 4));
        }
        let cast = command(
            &game,
            A,
            GameCommand::CastWithResources {
                spell: physical(&game, 1).id(),
                target: None,
                payment: CastingPayment {
                    additional: buyback.then(|| (0, 1.into())).into_iter().collect(),
                    mana: Some(spend(&payment)),
                    ..CastingPayment::default()
                },
            },
        );
        let paid = cast.mana_payment().unwrap();
        assert_eq!(paid.payer, A);
        assert_eq!(paid.cost.required, [(ManaType::Black, 1.into())].into());
        assert_eq!(paid.cost.generic, if buyback { 4 } else { 0 }.into());
        assert_eq!(cast.state().execution().mana(A, ManaType::Black), 0.into());
        assert_eq!(
            cast.state().execution().mana(A, ManaType::Colourless),
            if buyback { 0 } else { 4 }.into()
        );
        let stack_card = physical(cast.state(), 1);
        let sheet = cast
            .state()
            .execution()
            .current_characteristics(stack_card.id())
            .unwrap()
            .unwrap();
        assert!(
            matches!(sheet.current_names.iter().next(), Some(name) if name.as_str() == "Lab Rats")
        );
        assert_eq!(sheet.current_colors, [Color::Black].into());
        assert_eq!(stack_card.controller(), Some(A));
        let entry = &cast.state().execution().window().entries()[0];
        assert_eq!(entry.bought_back(), buyback);
        let done = if counter {
            let priority = pass(cast.state());
            let reply = targeted(priority.state(), B, 2, TargetRef::Stack(entry.id()));
            round(reply.into_state())
        } else {
            round(cast.into_state())
        };
        assert_eq!(physical(done.state(), 1).zone(), destination);
        assert!(done.state().execution().window().entries().is_empty());
        if counter {
            tokens(&done, A, 0);
            assert!(
                done.state()
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .all(|o| !o.is_token())
            );
        } else {
            let ids = tokens(&done, A, 1);
            assert_creature(
                done.state(),
                ids[0],
                ("Rat Token", &["Rat"], 1, 1),
                &[CardType::Creature],
                &[],
            );
        }
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn merciless_executioner_collects_apnap_choices_before_one_sacrifice_batch() {
    let bare = Program::creature(None, 1.into(), 5.into(), BTreeSet::new());
    for chosen in [CardId(1), CardId(2)] {
        let initial = game(
            vec![A, B, C],
            vec![
                source(EXECUTIONER, A, ObjectDestination::Hand),
                seed(
                    2,
                    B,
                    ObjectDestination::Battlefield { controller: A },
                    bare.clone(),
                ),
                seed(
                    3,
                    C,
                    ObjectDestination::Battlefield { controller: B },
                    bare.clone()
                        .with_printed_keywords([KeywordAbility::Indestructible]),
                ),
                seed(
                    4,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    Program::artifact(None, vec![]),
                ),
            ],
            &[(ManaType::Black, 1), (ManaType::Colourless, 2)],
            Step::PrecombatMain,
        );
        let cast = cast_untargeted(
            &initial,
            1,
            &[(ManaType::Black, 1), (ManaType::Colourless, 2)],
        );
        let cost = &cast.mana_payment().unwrap().cost;
        assert_eq!(cost.generic, 2.into());
        assert_eq!(cost.required, [(ManaType::Black, 1.into())].into());
        let entered = round(cast.into_state()).into_state();
        assert_creature(
            &entered,
            physical(&entered, 1).id(),
            ("Merciless Executioner", &["Orc", "Warrior"], 3, 1),
            &[CardType::Creature],
            &[],
        );
        let [trigger] = entered.execution().window().entries() else {
            panic!("one ETB trigger")
        };
        assert_eq!(trigger.controller(), A);
        assert!(
            matches!(trigger.kind(), StackKind::TriggeredAbility { source } if source.id() == physical(&entered, 1).id())
        );
        assert_eq!(
            entered.execution().stack_body(trigger.id()).unwrap().target,
            BodyTarget::None
        );
        let selecting = round(entered).into_state();
        let chosen = physical(&selecting, chosen.0).id();
        let theirs = physical(&selecting, 3).id();
        let first = selecting.pending_decision().unwrap();
        let RuleFrame::ResolutionSelect(selection) = first.frame() else {
            panic!("sacrifice choice")
        };
        assert_eq!(first.chooser(), A);
        assert_eq!(selection.operation(), ResolutionSelectionKind::Sacrifice);
        assert_eq!(selection.required(), 1);
        assert_eq!(
            selection.domain().iter().copied().collect::<BTreeSet<_>>(),
            [physical(&selecting, 1).id(), physical(&selecting, 2).id()].into()
        );
        let wrong = selecting.advance(GameInput {
            basis: selecting.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: first.id(),
                choice: DecisionReply::SelectResolution(vec![theirs]),
            },
        });
        assert!(matches!(
            wrong,
            Err(GameError::Effect(EffectError::SelectionOutsideDomain))
        ));
        let first = reply(&selecting, DecisionReply::SelectResolution(vec![chosen]));
        let second = first.state().pending_decision().unwrap();
        let RuleFrame::ResolutionSelect(selection) = second.frame() else {
            panic!("second sacrifice choice")
        };
        assert_eq!(second.chooser(), B);
        assert_eq!(selection.required(), 1);
        assert_eq!(selection.domain(), &[theirs]);
        assert!(
            events(&first)
                .flat_map(|e| &e.occurrences)
                .all(|row| { !matches!(&row.value, RulesOccurrence::ZoneChange(_)) })
        );
        let finished = reply(first.state(), DecisionReply::SelectResolution(vec![theirs]));
        assert!(finished.state().pending_decision().is_none());
        assert!(finished.state().execution().window().entries().is_empty());
        let batches = events(&finished)
            .map(|event| {
                event
                    .occurrences
                    .iter()
                    .filter_map(|row| match &row.value {
                        RulesOccurrence::ZoneChange(change)
                            if matches!(change.reason, ZoneReason::Sacrifice { .. }) =>
                        {
                            Some(change)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|batch| !batch.is_empty())
            .collect::<Vec<_>>();
        let [batch] = batches.as_slice() else {
            panic!("one simultaneous sacrifice event")
        };
        let [a, b] = batch.as_slice() else {
            panic!("exactly two sacrifices")
        };
        for (moved, before, player) in [(*a, chosen, A), (*b, theirs, B)] {
            assert_eq!(moved.before.object.id(), before);
            assert_eq!(moved.reason, ZoneReason::Sacrifice { player });
            assert_eq!(
                moved.after.object.zone(),
                ObjectZone::Graveyard(moved.before.object.owner())
            );
        }
        let spared = if chosen == physical(&selecting, 1).id() {
            2
        } else {
            1
        };
        assert_eq!(
            physical(finished.state(), spared).zone(),
            ObjectZone::Battlefield
        );
    }
}

fn sacrifice_each() -> Instruction {
    Instruction::Sacrifice {
        players: AffectedPlayers::EachPlayer,
        amount: spellwake_engine::program::ValueExpr::Literal(1.into()),
        filter: PermanentTypeFilter {
            any_types: Default::default(),
            required_types: [CardType::Creature].into(),
            forbidden_types: BTreeSet::new(),
        },
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn wurmcoil_larva_death_uses_last_controller_and_creates_both_tokens_together() {
    let larva = source(LARVA, A, ObjectDestination::Hand);
    let mut escape = free_instant(
        BodyTarget::None,
        Instruction::DrawController {
            amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
        },
    );
    escape.conditional_cost = Some(ConditionalCastCost::Escape {
        mana: ManaCost::free(),
        exile: 1,
        entry_counters: 0,
    });
    for exile in [false, true] {
        let seize = free_instant(
            BodyTarget::SinglePermanent,
            Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                    ControlRecipient::ResolvingController,
                )]),
                duration: ResolvedDuration::NoStatedDuration,
            },
        );
        let mut cards = vec![
            larva.clone(),
            seed(2, B, ObjectDestination::Hand, seize),
            seed(
                3,
                A,
                ObjectDestination::Hand,
                free_instant(BodyTarget::None, sacrifice_each()),
            ),
            seed(4, A, ObjectDestination::Graveyard, escape.clone()),
        ];
        if exile {
            let mut replacement = Program::enchantment(None);
            replacement.zone_replacements.push(ZoneReplacementAbility {
                id: ZoneReplacementAbilityId(0),
                actions: ZoneActionPredicate::OneOf([ZoneActionKind::EffectSacrifice].into()),
                objects: ZoneObjectKind::Card,
                subject: ZoneReplacementSubject::AnyObject,
                destination: PublicZoneDestination::Graveyard,
                replace_with: PublicZoneDestination::Exile,
                choice: ZoneReplacementChoice::Mandatory,
            });
            cards.push(seed(
                5,
                A,
                ObjectDestination::Battlefield { controller: A },
                replacement,
            ));
        }
        let game = game(
            vec![A, B],
            cards,
            &[(ManaType::Colourless, 3), (ManaType::Black, 2)],
            Step::PrecombatMain,
        );
        let cast = cast_untargeted(&game, 1, &[(ManaType::Black, 2), (ManaType::Colourless, 3)]);
        assert_eq!(cast.mana_payment().unwrap().cost.generic, 3.into());
        assert_eq!(
            cast.mana_payment().unwrap().cost.required,
            [(ManaType::Black, 2.into())].into()
        );
        let entered = round(cast.into_state()).into_state();
        let larva = physical(&entered, 1).id();
        assert_creature(
            &entered,
            larva,
            ("Wurmcoil Larva", &["Phyrexian", "Wurm"], 3, 3),
            &[CardType::Artifact, CardType::Creature],
            &[KeywordAbility::Deathtouch, KeywordAbility::Lifelink],
        );
        let priority = pass(&entered);
        let seize = targeted(priority.state(), B, 2, TargetRef::Object(larva));
        let stolen = round(seize.into_state()).into_state();
        assert_eq!(physical(&stolen, 1).owner(), A);
        assert_eq!(physical(&stolen, 1).controller(), Some(B));
        let choosing = round(cast_untargeted(&stolen, 3, &[]).into_state()).into_state();
        assert_eq!(choosing.pending_decision().unwrap().chooser(), B);
        let death = reply(&choosing, DecisionReply::SelectResolution(vec![larva]));
        if exile {
            assert_eq!(physical(death.state(), 1).zone(), ObjectZone::Exile);
            assert!(death.state().execution().window().entries().is_empty());
            tokens(&death, B, 0);
            continue;
        }
        assert_eq!(physical(death.state(), 1).zone(), ObjectZone::Graveyard(A));
        let [trigger] = death.state().execution().window().entries() else {
            panic!("one death trigger")
        };
        assert_eq!(trigger.controller(), B);
        assert!(
            matches!(trigger.kind(), StackKind::TriggeredAbility { source } if source.id() == larva)
        );
        let entry = trigger.id();
        let exiled = command(
            death.state(),
            A,
            GameCommand::CastWithResources {
                spell: physical(death.state(), 4).id(),
                target: None,
                payment: CastingPayment {
                    use_alternative: true,
                    objects: vec![CastResource::Escape(physical(death.state(), 1).id())],
                    ..CastingPayment::default()
                },
            },
        );
        assert_eq!(physical(exiled.state(), 1).zone(), ObjectZone::Exile);
        let ordering = round(round(exiled.into_state()).into_state());
        let pending = ordering.state().pending_decision().unwrap();
        let RuleFrame::TimestampOrder(order) = pending.frame() else {
            panic!("simultaneous token timestamp order")
        };
        assert_eq!(pending.chooser(), B);
        assert_eq!(order.entry(), entry);
        assert_eq!(order.members().len(), 2);
        tokens(&ordering, B, 0);
        let done = reply(
            ordering.state(),
            DecisionReply::TimestampOrder(order.members().iter().map(|m| m.member).collect()),
        );
        assert!(events(&done).any(|event| {
            event.cause
                == EventCause::Instruction {
                    entry,
                    instruction: 0,
                }
                && event
                    .occurrences
                    .iter()
                    .filter(|row| matches!(row.value, RulesOccurrence::TokenCreated { .. }))
                    .count()
                    == 2
        }));
        let ids = tokens(&done, B, 2);
        for (id, power, toughness, keyword) in [
            (ids[0], 1, 2, KeywordAbility::Deathtouch),
            (ids[1], 2, 1, KeywordAbility::Lifelink),
        ] {
            assert_creature(
                done.state(),
                id,
                (
                    "Phyrexian Wurm Token",
                    &["Phyrexian", "Wurm"],
                    power,
                    toughness,
                ),
                &[CardType::Artifact, CardType::Creature],
                &[keyword],
            );
        }
    }
}

fn combat_frame(game: &GameState, attackers: bool) -> GameState {
    let mut game = game.clone();
    for _ in 0..24 {
        match game.pending_decision().map(|p| p.frame()) {
            Some(RuleFrame::Combat(CombatDecision::Attackers(_))) if attackers => return game,
            Some(RuleFrame::Combat(CombatDecision::Blockers(_))) if !attackers => return game,
            Some(frame) => panic!("unexpected combat decision {frame:?}"),
            None => game = pass(&game).into_state(),
        }
    }
    panic!("combat decision was not offered")
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn wurmcoil_larva_applies_deathtouch_and_lifelink_to_the_same_damage() {
    for (indestructible, prevented) in [(false, false), (true, false), (false, true)] {
        let mut attacker = Program::creature(None, 1.into(), 5.into(), BTreeSet::new());
        if indestructible {
            attacker
                .printed_keywords
                .insert(KeywordAbility::Indestructible);
        }
        let mut cards = vec![
            source(LARVA, A, ObjectDestination::Battlefield { controller: B }),
            seed(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                attacker,
            ),
        ];
        if prevented {
            let shield = free_instant(
                BodyTarget::SingleCreature,
                Instruction::CreatePreventionShield { amount: 3.into() },
            );
            cards.push(seed(3, A, ObjectDestination::Hand, shield));
        }
        let mut game = game(vec![A, B], cards, &[], Step::Untap);
        if prevented {
            let shield = targeted(&game, A, 3, TargetRef::Object(physical(&game, 2).id()));
            game = round(shield.into_state()).into_state();
        }
        let larva = physical(&game, 1).id();
        let attacker = physical(&game, 2).id();
        let attacks = combat_frame(&game, true);
        let attacked = reply(
            &attacks,
            DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker,
                defender: B,
                defending_planeswalker: None,
            }]),
        );
        let blocks = combat_frame(attacked.state(), false);
        let blocked = reply(
            &blocks,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: larva,
                attacker,
            }]),
        );
        let damage = round(blocked.into_state());
        let larva_damage = damage.damage_journal().applied.iter().find(|dealt| {
            dealt.source.id() == DamageSourceId::Object(larva)
                && dealt.target == DamageRecipient::Creature(attacker)
        });
        if prevented {
            assert!(larva_damage.is_none());
        } else {
            let dealt = larva_damage.unwrap();
            assert_eq!(dealt.source.controller(), B);
            assert_eq!(dealt.amount, 3.into());
        }
        let life = if prevented { 20 } else { 23 };
        assert_eq!(damage.state().execution().life_total(B), Some(&life.into()));
        assert_eq!(damage.state().execution().life_total(A), Some(&20.into()));
        assert_eq!(
            damage.state().execution().battlefield_marked_damage(larva),
            Some(1.into())
        );
        let expected = if indestructible || prevented {
            ObjectZone::Battlefield
        } else {
            ObjectZone::Graveyard(A)
        };
        assert_eq!(physical(damage.state(), 2).zone(), expected);
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn cmm_sliver_stat_lords_apply_to_correct_controllers_and_end_on_departure() {
    let (_, vocabulary, registry) = current_catalogue();
    let sliver = vocabulary.resolve_label("Sliver").unwrap();
    struct Case {
        name: &'static str,
        generic: i64,
        color: ManaType,
        body: (i64, i64),
        bonus: (i64, i64),
        controlled_only: bool,
    }
    let cases = [
        Case {
            name: "Blade Sliver",
            generic: 2,
            color: ManaType::Red,
            body: (2, 2),
            bonus: (1, 0),
            controlled_only: false,
        },
        Case {
            name: "Bonesplitter Sliver",
            generic: 3,
            color: ManaType::Red,
            body: (2, 2),
            bonus: (2, 0),
            controlled_only: false,
        },
        Case {
            name: "Sinew Sliver",
            generic: 1,
            color: ManaType::White,
            body: (1, 1),
            bonus: (1, 1),
            controlled_only: false,
        },
        Case {
            name: "Might Sliver",
            generic: 4,
            color: ManaType::Green,
            body: (2, 2),
            bonus: (2, 2),
            controlled_only: false,
        },
        Case {
            name: "Cleaving Sliver",
            generic: 3,
            color: ManaType::Red,
            body: (2, 2),
            bonus: (2, 0),
            controlled_only: true,
        },
        Case {
            name: "Megantic Sliver",
            generic: 5,
            color: ManaType::Green,
            body: (3, 3),
            bonus: (3, 3),
            controlled_only: true,
        },
    ];
    for Case {
        name,
        generic,
        color,
        body: (power, toughness),
        bonus: (bonus_p, bonus_t),
        controlled_only,
    } in cases
    {
        let row = current_catalogue()
            .0
            .cards
            .iter()
            .find(|r| r.name == name)
            .unwrap();
        let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        let named = registry
            .seed(&oracle, CardId(1), A, ObjectDestination::Hand)
            .unwrap();
        let peer = || Program::creature(None, 4.into(), 4.into(), [sliver].into());
        let other = Program::creature(None, 4.into(), 4.into(), BTreeSet::new());
        let removal = free_instant(BodyTarget::SingleCreature, Instruction::DestroyTarget);
        let initial = game(
            vec![A, B],
            vec![
                named,
                seed(
                    2,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    peer(),
                ),
                seed(
                    3,
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    peer(),
                ),
                seed(
                    4,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    other,
                ),
                seed(5, A, ObjectDestination::Hand, removal),
            ],
            &[(ManaType::Colourless, generic), (color, 1)],
            Step::PrecombatMain,
        );
        let stats = |state: &GameState, id| {
            state
                .execution()
                .current_characteristics(physical(state, id).id())
                .unwrap()
                .unwrap()
                .power_toughness
                .unwrap()
                .derived
        };
        assert_eq!(
            stats(&initial, 2),
            PowerToughness {
                power: 4.into(),
                toughness: 4.into()
            }
        );
        let casting = cast_untargeted(&initial, 1, &[(ManaType::Colourless, generic), (color, 1)])
            .into_state();
        let entered = round(casting).into_state();
        let source_sheet = entered
            .execution()
            .current_characteristics(physical(&entered, 1).id())
            .unwrap()
            .unwrap();
        assert_eq!(
            source_sheet
                .current_names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>(),
            [name]
        );
        assert_eq!(source_sheet.current_types, [CardType::Creature].into());
        assert_eq!(source_sheet.current_creature_subtypes, [sliver].into());
        let expected_color = match color {
            ManaType::Red => Color::Red,
            ManaType::Green => Color::Green,
            ManaType::White => Color::White,
            _ => unreachable!(),
        };
        assert_eq!(source_sheet.current_colors, [expected_color].into());
        assert_eq!(
            stats(&entered, 1),
            PowerToughness {
                power: (power + bonus_p).into(),
                toughness: (toughness + bonus_t).into()
            },
            "{name}"
        );
        assert_eq!(
            stats(&entered, 2),
            PowerToughness {
                power: (4 + bonus_p).into(),
                toughness: (4 + bonus_t).into()
            },
            "{name}"
        );
        let opponent_bonus = if controlled_only {
            (0, 0)
        } else {
            (bonus_p, bonus_t)
        };
        assert_eq!(
            stats(&entered, 3),
            PowerToughness {
                power: (4 + opponent_bonus.0).into(),
                toughness: (4 + opponent_bonus.1).into()
            },
            "{name}"
        );
        assert_eq!(
            stats(&entered, 4),
            PowerToughness {
                power: 4.into(),
                toughness: 4.into()
            },
            "{name}"
        );
        let removal_on_stack = targeted(
            &entered,
            A,
            5,
            TargetRef::Object(physical(&entered, 1).id()),
        )
        .into_state();
        let gone = round(removal_on_stack).into_state();
        assert_eq!(
            physical(&gone, 1).zone(),
            ObjectZone::Graveyard(A),
            "{name}"
        );
        assert_eq!(
            stats(&gone, 2),
            PowerToughness {
                power: 4.into(),
                toughness: 4.into()
            },
            "{name}"
        );
        assert_eq!(
            stats(&gone, 3),
            PowerToughness {
                power: 4.into(),
                toughness: 4.into()
            },
            "{name}"
        );
    }
}

#[test]
#[ignore = "requires dated Oracle source and creature vocabulary"]
fn sliver_stat_parser_rejects_unrepresented_additional_text_and_bad_modifiers() {
    let (inventory, vocabulary, _) = current_catalogue();
    let original = inventory
        .cards
        .iter()
        .find(|row| row.name == "Blade Sliver")
        .unwrap();
    for text in [
        "All Sliver creatures get +1/+0.\nFlying",
        "All Sliver creatures get +1/+0 until end of turn.",
        "All Sliver creatures get -1/+0.",
        "All Sliver creatures get +1/+0. Draw a card.",
    ] {
        let mut altered = original.clone();
        altered.faces[0].oracle_text = Some(text.to_owned());
        assert!(
            static_grants::sliver_stats(&altered, &vocabulary).is_none(),
            "{text}"
        );
    }
}
mod cmm_copy_control;
mod cmm_gameplay;
mod cmm_permanent_costs;
mod cmm_walkers_combat;
mod disturb;
mod meld;
mod mutate;
mod split_spells;

mod avacyn_auras;
mod avacyn_combat;
mod avacyn_miracles;
mod avacyn_rules;
mod phasing;
mod soulbond;

mod banding;
mod splice;
