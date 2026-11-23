//! Names and actual colors come from both faces of the pinned Oracle records.
use super::*;
use spellwake_engine::{
    combat::CombatDecision,
    day_night::DayNight,
    game::{DecisionReply, NativeCandidate, RuleFrame},
    land::BasicLandType,
    mana::ManaAbilityRef,
    object_identity::ObjectId,
    occurrence::{EventCause, RulesOccurrence, RulesRecord},
    targets::TargetRef,
};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn id(s: &GameState, n: u64) -> ObjectId {
    s.execution()
        .window()
        .cards()
        .current_identity(CardId(n))
        .unwrap()
}
fn step(s: &GameState, actor: SeatId, command: GameCommand) -> NativeCandidate {
    s.advance(GameInput {
        basis: s.basis().clone(),
        actor,
        command,
    })
    .unwrap()
}
fn pass(s: &GameState) -> NativeCandidate {
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) = s.execution().window().phase()
    else {
        panic!("expected priority {:?}", s.pending_decision())
    };
    step(s, holder, GameCommand::Pass)
}
fn reply(s: &GameState, choice: DecisionReply) -> NativeCandidate {
    let p = s.pending_decision().unwrap();
    step(
        s,
        p.chooser(),
        GameCommand::Reply {
            decision: p.id(),
            choice,
        },
    )
}
fn until(mut s: GameState, reached: impl Fn(&GameState) -> bool) -> GameState {
    for _ in 0..150 {
        if reached(&s) {
            return s;
        }
        s = if let Some(p) = s.pending_decision() {
            let choice = match p.frame() {
                RuleFrame::Combat(CombatDecision::Attackers(_)) => {
                    DecisionReply::DeclareAttackers(vec![])
                }
                other => panic!("unexpected {other:?}"),
            };
            reply(&s, choice).into_state()
        } else {
            pass(&s).into_state()
        };
    }
    panic!("requested boundary never arrived")
}
fn at(s: &GameState, who: SeatId, when: Step) -> bool {
    s.execution().window().active_player() == who
        && s.turn().unwrap().position().step == when
        && s.pending_decision().is_none()
}
fn paid(color: ManaType, n: i64) -> Option<ManaPayment> {
    Some(ManaPayment {
        from_lots: Default::default(),
        spend: vec![ManaSpend {
            mana_type: color,
            amount: n.into(),
        }],
    })
}
fn resolve(s: &GameState) -> NativeCandidate {
    pass(pass(s).state())
}
fn verify_face(
    s: &GameState,
    who: u64,
    back: bool,
    name: &str,
    color: Color,
    power: i64,
    toughness: i64,
) {
    let object = id(s, who);
    assert_eq!(
        s.execution()
            .window()
            .cards()
            .get(object)
            .unwrap()
            .transformed(),
        back
    );
    let evaluated = s.execution().evaluated_characteristics().unwrap();
    let view = evaluated.get(object).unwrap();
    assert_eq!(view.current_names.iter().next().unwrap().as_str(), name);
    assert_eq!(view.current_colors, [color].into());
    let stats = &view.power_toughness.as_ref().unwrap().derived;
    assert_eq!(
        (stats.power.clone(), stats.toughness.clone()),
        (power.into(), toughness.into())
    );
    assert!(view.has_keyword(if back {
        KeywordAbility::Nightbound
    } else {
        KeywordAbility::Daybound
    }));
    assert_eq!(s.execution().permanent_mana_value(object), Some(3.into()));
}
#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn bird_admirer_and_harvesttide_infiltrator_transform_before_upkeep_enter_at_night_and_retain_last_face()
 {
    let (inventory, vocab, reg) = current_catalogue();
    let interaction = Registry::from_interaction_inventory(inventory, &vocab).unwrap();
    let bird = "Bird Admirer // Wing Shredder";
    let wolf = "Harvesttide Infiltrator // Harvesttide Assailant";
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let ctx = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocab.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "day-night".into(),
        "printed-faces".into(),
        vocab.id(),
        names.id(),
    )
    .unwrap();
    let home = |controller| ObjectDestination::Battlefield { controller };
    let mut cards = vec![];
    let mut order = vec![];
    for (n, name, owner, zone) in [
        (1, bird, A, home(A)),
        (2, wolf, A, home(A)),
        (3, wolf, B, ObjectDestination::Hand),
        (4, "Ornithopter", B, ObjectDestination::Hand),
        (5, "Unsummon", A, ObjectDestination::Hand),
        (6, "Island", A, home(A)),
        (7, "Mountain", B, home(B)),
        (8, "Mountain", B, home(B)),
        (9, "Mountain", B, home(B)),
        (10, "Island", A, ObjectDestination::LibraryBottom),
        (11, "Mountain", B, ObjectDestination::LibraryBottom),
        (12, "Island", A, ObjectDestination::LibraryBottom),
        (13, "Mountain", B, ObjectDestination::LibraryBottom),
    ] {
        let row = inventory
            .cards
            .iter()
            .find(|r| r.required_by_base_formats && r.name == name)
            .unwrap();
        let oracle = OracleId::try_from(row.oracle_id.clone().unwrap()).unwrap();
        let source = if name == "Unsummon" {
            &interaction
        } else {
            reg
        };
        let card = source
            .seed(&oracle, CardId(n), owner, zone)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        if matches!(zone, ObjectDestination::Battlefield { .. }) {
            order.push(card.card);
        }
        cards.push(card);
    }
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        order,
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocab,
        names,
    );
    let mut s = GameState::initialize_scheduled(
        GameId(731),
        ctx,
        seed,
        TurnStart {
            number: 1,
            step: Step::Untap,
        },
    )
    .unwrap()
    .into_state();
    assert_eq!(s.execution().day_night(), DayNight::Day);
    let original = id(&s, 1);
    verify_face(&s, 1, false, "Bird Admirer", Color::Green, 1, 4);
    s = until(s, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::TransformOrder(_))
        )
    });
    let p = s.pending_decision().unwrap();
    let RuleFrame::TransformOrder(f) = p.frame() else {
        unreachable!()
    };
    assert_eq!(f.chooser(), A);
    assert_eq!(f.new_designation(), DayNight::Night);
    assert_eq!(s.turn().unwrap().position().step, Step::Untap);
    assert_eq!(s.execution().day_night(), DayNight::Day);
    assert!(
        s.advance(GameInput {
            basis: s.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: p.id(),
                choice: DecisionReply::TransformOrder(vec![original, original])
            }
        })
        .is_err()
    );
    let moved = reply(
        &s,
        DecisionReply::TransformOrder(vec![id(&s, 2), id(&s, 1)]),
    );
    assert!(moved.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if matches!(e.cause,EventCause::UntapTurn{active:B}) && e.occurrences.iter().filter(|o|matches!(o.value,RulesOccurrence::Transformed{..})).count()==2)));
    s = moved.into_state();
    assert_eq!(s.execution().day_night(), DayNight::Night);
    assert_eq!(id(&s, 1), original);
    verify_face(&s, 1, true, "Wing Shredder", Color::Green, 3, 5);
    s = until(s, |s| at(s, B, Step::PrecombatMain));
    for n in [7, 8, 9] {
        s = step(
            &s,
            B,
            GameCommand::ActivateMana {
                source: id(&s, n),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Mountain),
            },
        )
        .into_state();
    }
    s = step(
        &s,
        B,
        GameCommand::CastUntargeted {
            spell: id(&s, 3),
            payment: paid(ManaType::Red, 3),
        },
    )
    .into_state();
    let entered = resolve(&s);
    let newbody = id(entered.state(), 3);
    assert!(entered.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(m) if m.after.object.id()==newbody && m.after.object.transformed())))));
    s = entered.into_state();
    verify_face(&s, 3, true, "Harvesttide Assailant", Color::Red, 4, 4);
    // At the same precombat priority window the opponent can return the back face.
    let mut bounced = pass(&s).into_state();
    bounced = step(
        &bounced,
        A,
        GameCommand::ActivateMana {
            source: id(&bounced, 6),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        },
    )
    .into_state();
    bounced = step(
        &bounced,
        A,
        GameCommand::CastPaid {
            spell: id(&bounced, 5),
            target: TargetRef::Object(newbody),
            payment: paid(ManaType::Blue, 1).unwrap(),
        },
    )
    .into_state();
    let gone = resolve(&bounced);
    let now = id(gone.state(), 3);
    assert_ne!(now, newbody);
    assert!(gone.rules_journal().records.iter().any(|r|matches!(r,RulesRecord::Event(e) if e.occurrences.iter().any(|o|matches!(&o.value,RulesOccurrence::ZoneChange(m) if m.before.object.id()==newbody && m.before.object.transformed() && m.before.current_names.iter().next().unwrap().as_str()=="Harvesttide Assailant" && m.after.object.id()==now && !m.after.object.transformed() && m.after.current_names.iter().next().unwrap().as_str()=="Harvesttide Infiltrator")))));
    // In the first branch B cast twice. The controller orders A's two permanents,
    // and B's one permanent changes at the same time.
    s = step(
        &s,
        B,
        GameCommand::CastUntargeted {
            spell: id(&s, 4),
            payment: None,
        },
    )
    .into_state();
    s = resolve(&s).into_state();
    s = until(s, |s| {
        matches!(
            s.pending_decision().map(|p| p.frame()),
            Some(RuleFrame::TransformOrder(_))
        )
    });
    let f = s.pending_decision().unwrap();
    let RuleFrame::TransformOrder(f) = f.frame() else {
        unreachable!()
    };
    assert_eq!(f.new_designation(), DayNight::Day);
    let done = reply(&s, DecisionReply::TransformOrder(f.objects().to_vec()));
    verify_face(done.state(), 1, false, "Bird Admirer", Color::Green, 1, 4);
    verify_face(
        done.state(),
        3,
        false,
        "Harvesttide Infiltrator",
        Color::Red,
        3,
        2,
    );
    assert_eq!(done.state().execution().day_night(), DayNight::Day);
    // In the other branch A's response does not contribute to B's single cast.
    bounced = until(gone.into_state(), |s| at(s, A, Step::Upkeep));
    assert_eq!(bounced.execution().day_night(), DayNight::Night);
}

#[test]
#[ignore = "requires pinned Oracle and creature vocabulary inputs"]
fn original_werewolf_upkeeps_count_the_spells_of_each_player() {
    let (inventory, vocabulary) = dated_sources();
    let selected = inventory
        .cards
        .iter()
        .filter(|card| {
            card.required_by_base_formats
                && matches!(
                    card.name.as_str(),
                    "Reckless Waif // Merciless Predator" | "Mountain"
                )
        })
        .filter_map(|card| card.oracle_id.clone())
        .collect();
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&selected),
    )
    .unwrap();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "transform".into(),
        "previous-turn-casts".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let card = |n, name: &str, owner, zone| {
        let id = inventory
            .cards
            .iter()
            .find(|card| card.required_by_base_formats && card.name == name)
            .and_then(|card| card.oracle_id.clone())
            .unwrap();
        registry
            .seed(&OracleId::try_from(id).unwrap(), CardId(n), owner, zone)
            .unwrap()
    };
    let blank = |n, owner| SeedCard {
        card: CardId(n),
        owner,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            spellwake_engine::mana::ManaCost::free(),
            spellwake_engine::program::ExecutableBody::new(
                spellwake_engine::program::BodyTarget::None,
                vec![spellwake_engine::program::Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::Literal(0.into()),
                }],
            ),
        ),
    };
    let mut cards = vec![
        card(
            1,
            "Reckless Waif // Merciless Predator",
            A,
            ObjectDestination::Battlefield { controller: A },
        ),
        blank(2, A),
        blank(3, A),
        blank(4, B),
        blank(5, A),
    ];
    for n in 6..14 {
        cards.push(card(
            n,
            "Mountain",
            if n % 2 == 0 { A } else { B },
            ObjectDestination::LibraryBottom,
        ));
    }
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let mut game = GameState::initialize_scheduled(
        GameId(734),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let identity = id(&game, 1);
    let cast_zero = |game: &GameState, player, n| {
        let cast = step(
            game,
            player,
            GameCommand::CastUntargeted {
                spell: id(game, n),
                payment: None,
            },
        );
        resolve(cast.state()).into_state()
    };
    let transformed = |game: &GameState| {
        assert_eq!(id(game, 1), identity);
        game.execution()
            .window()
            .cards()
            .get(identity)
            .unwrap()
            .transformed()
    };

    game = until(game, |game| at(game, B, Step::Upkeep));
    assert_eq!(game.execution().window().entries().len(), 1);
    assert!(!transformed(&game));
    game = resolve(&game).into_state();
    assert!(transformed(&game));
    assert_eq!(game.execution().day_night(), DayNight::Neither);

    game = cast_zero(&game, B, 4);
    game = pass(&game).into_state();
    game = cast_zero(&game, A, 2);
    game = until(game, |game| at(game, A, Step::Upkeep));
    assert!(game.execution().window().entries().is_empty());
    assert!(transformed(&game));

    game = cast_zero(&game, A, 3);
    game = cast_zero(&game, A, 5);
    game = until(game, |game| at(game, B, Step::Upkeep));
    assert_eq!(game.execution().window().entries().len(), 1);
    game = resolve(&game).into_state();
    assert!(!transformed(&game));
    assert_eq!(
        game.execution().permanent_mana_value(identity),
        Some(1.into())
    );
}

#[test]
#[ignore = "requires explicit saved Oracle catalogue source"]
fn original_transform_choices_reveal_without_moving_and_use_starting_life() {
    use spellwake_engine::{
        activated::ActivatedAbilityId,
        effects::LibraryTopReply,
        life::LifeAdjustment,
        program::{BodyTarget, ExecutableBody, Instruction},
    };
    let (inventory, vocabulary) = dated_sources();
    let names = Arc::new(spellwake_contracts::names::NameRules::literal_names());
    let oracle = |name: &str| {
        inventory
            .cards
            .iter()
            .find(|card| card.required_by_base_formats && card.name == name)
            .and_then(|card| card.oracle_id.clone())
            .unwrap()
    };
    let delver = oracle("Delver of Secrets // Insectile Aberration");
    let chalice = oracle("Chalice of Life // Chalice of Death");
    let registry = Registry::compile_keyword_selection(
        inventory,
        &vocabulary,
        Profile::Current,
        Some(&[delver.clone(), chalice.clone()].into()),
    )
    .unwrap();
    let mana = spellwake_engine::mana::ManaCost::free;
    let gain = |amount| {
        Program::instant(
            mana(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(amount),
                }],
            ),
        )
    };
    let context = GameContext::new(
        CURRENT_KEYWORD_PROFILE.into(),
        vocabulary.rules_revision().into(),
        TEACHING_SOURCE_SHA256.into(),
        "transform".into(),
        "reveal-and-life".into(),
        vocabulary.id(),
        names.id(),
    )
    .unwrap();
    let cards = vec![
        registry
            .seed(
                &OracleId::try_from(delver).unwrap(),
                CardId(1),
                A,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap(),
        registry
            .seed(
                &OracleId::try_from(chalice).unwrap(),
                CardId(2),
                A,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap(),
        SeedCard {
            card: CardId(3),
            owner: A,
            destination: ObjectDestination::Hand,
            program: gain(9.into()),
        },
        SeedCard {
            card: CardId(4),
            owner: A,
            destination: ObjectDestination::LibraryBottom,
            program: gain(1.into()),
        },
        SeedCard {
            card: CardId(5),
            owner: B,
            destination: ObjectDestination::LibraryBottom,
            program: gain(1.into()),
        },
    ];
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1), CardId(2)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        names,
    );
    let game = GameState::initialize_scheduled(
        GameId(735),
        context,
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let cast = step(
        &game,
        A,
        GameCommand::CastUntargeted {
            spell: id(&game, 3),
            payment: None,
        },
    );
    let game = resolve(cast.state()).into_state();
    assert_eq!(game.execution().life_total(A), Some(&29.into()));
    let ability = step(
        &game,
        A,
        GameCommand::ActivateUntargetedAbility {
            source: id(&game, 2),
            ability: ActivatedAbilityId(0),
            payment: None,
        },
    );
    let transformed = |game: &GameState, card| {
        game.execution()
            .window()
            .cards()
            .get(id(game, card))
            .unwrap()
            .transformed()
    };
    let game = resolve(ability.state()).into_state();
    assert_eq!(game.execution().life_total(A), Some(&30.into()));
    assert!(transformed(&game, 2));

    let upkeep = until(game, |game| at(game, A, Step::Upkeep));
    let mut choice = upkeep;
    while choice.pending_decision().is_none() {
        choice = pass(&choice).into_state();
    }
    let RuleFrame::LibraryTop(frame) = choice.pending_decision().unwrap().frame() else {
        panic!("Delver must offer a private reveal choice");
    };
    assert!(frame.is_reveal_only());
    let card = id(&choice, 4);
    assert_eq!(frame.eligible_results(), &[card]);

    let accept = |selected| {
        reply(
            &choice,
            DecisionReply::LibraryTop(LibraryTopReply::Search { selected }),
        )
        .into_state()
    };
    let declined = accept(None);
    assert!(!transformed(&declined, 1));
    let revealed = accept(Some(card));
    assert!(transformed(&revealed, 1));
    assert_eq!(revealed.execution().window().cards().library(A), &[card]);
}
