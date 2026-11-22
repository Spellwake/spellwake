use super::*;

fn settle_three(mut game: GameState) -> GameState {
    for _ in 0..3 {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            game.execution().window().phase()
        else {
            panic!("all three players must have priority in turn before resolution");
        };
        game = step(&game, holder, GameCommand::Pass);
    }
    game
}

#[test]
fn opening_departure_preserves_the_surviving_mulligan_and_starter() {
    let cards = [A, B, C]
        .into_iter()
        .flat_map(|owner| {
            (0..15).map(move |index| {
                card(
                    100 + u64::from(owner.0) * 100 + index,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Program::artifact(None, vec![]),
                )
            })
        })
        .collect();
    let (context, seed) = seed(cards, vec![A, B, C], &[20, 20, 20]);
    let state = GameState::initialize_opening(
        GameId(1050),
        context,
        OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0; 300]),
        },
    )
    .unwrap()
    .into_state();
    assert_eq!(state.pending_decision().unwrap().chooser(), A);
    let state = step(&state, A, GameCommand::Concede);
    assert_eq!(state.execution().window().participants(), &[B, C]);
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    assert_eq!(state.execution().window().active_player(), B);
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Mulligan));
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Keep));
    assert_eq!(state.opening().unwrap().mulligans_taken(B), 1);
    assert_eq!(state.opening().unwrap().charged(B), 0);
    let state = reply(&state, DecisionReply::Mulligan(MulliganChoice::Keep));
    assert!(state.opening().unwrap().complete());
    assert_eq!(state.execution().window().active_player(), B);
    assert_eq!(state.turn().unwrap().position().number, 1);
}

#[test]
fn owner_leaving_during_a_stolen_spells_resolution_keeps_the_exact_tail() {
    let spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                },
                Instruction::ChooseCreatureType {
                    into: CreatureTypeSlot::new(0),
                    excluding: BTreeSet::new(),
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(2.into()),
                },
            ],
        ),
    );
    let steal = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::SingleSpell(crate::program::SpellFilter::noncreature()),
            vec![Instruction::GainControlOfSpell],
        ),
    );
    let state = root(
        vec![
            card(1, A, ObjectDestination::Hand, spell),
            card(2, B, ObjectDestination::Hand, steal),
        ],
        &[20, 20, 20],
        None,
        vec![A, B, C],
    );
    let state = step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    );
    let state = step(&state, A, GameCommand::Pass);
    let entry = state.execution().window().entries()[0].id();
    let state = step(
        &state,
        B,
        GameCommand::Cast {
            spell: object(&state, 2),
            target: TargetRef::Stack(entry),
        },
    );
    let state = settle_three(settle_three(state));
    let original = state.pending_decision().unwrap().id();
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    assert_eq!(state.execution().life_total(B), Some(&21.into()));
    let state = step(&state, A, GameCommand::Concede);
    assert!(state.execution().window().entries().is_empty());
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .is_none()
    );
    let decision = state.pending_decision().unwrap();
    let RuleFrame::CreatureType(frame) = decision.frame() else {
        panic!("preserved creature type instruction");
    };
    assert_eq!(frame.entry(), entry);
    assert_eq!(frame.instruction(), 1);
    assert_eq!(decision.chooser(), B);
    assert_ne!(decision.id(), original);
    let chosen = state
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    let state = reply(&state, DecisionReply::CreatureType(chosen));
    assert_eq!(state.execution().life_total(B), Some(&23.into()));
    assert!(state.pending_decision().is_none());
}

#[test]
fn trigger_order_preserves_its_wave_and_a_departed_chooser_loses_only_its_cohort() {
    let mut watcher = Program::artifact(None, vec![]);
    for id in [0, 1] {
        watcher.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: crate::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                crate::program::PlayerTriggerAction::Surveilled,
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
    }
    let spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::Surveil {
                player: ResolvingPlayer::Controller,
                amount: crate::program::ValueExpr::Literal(1.into()),
            }],
        ),
    );
    let state = root(
        vec![
            card(1, A, ObjectDestination::Hand, spell),
            card(
                2,
                A,
                ObjectDestination::Battlefield { controller: A },
                watcher,
            ),
        ],
        &[20, 20, 20],
        None,
        vec![A, B, C],
    );
    let state = settle_three(step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    ));
    let RuleFrame::Triggers(original) = state.pending_decision().unwrap().frame() else {
        panic!("started trigger wave");
    };
    let surviving = step(&state, C, GameCommand::Concede);
    let RuleFrame::Triggers(refreshed) = surviving.pending_decision().unwrap().frame() else {
        panic!("preserved trigger wave");
    };
    assert_eq!(refreshed.wave(), original.wave());
    assert_eq!(refreshed.instances(), original.instances());
    let ordered = reply(
        &surviving,
        DecisionReply::OrderTriggers(refreshed.instances().to_vec()),
    );
    assert_eq!(ordered.execution().window().entries().len(), 2);

    let departed = step(&state, A, GameCommand::Concede);
    assert_eq!(departed.execution().window().participants(), &[B, C]);
    assert!(departed.pending_decision().is_none());
    assert!(departed.execution().window().entries().is_empty());
}

#[test]
fn grouped_discard_keeps_accepted_selections_and_skips_the_departed_hand() {
    let spell = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::Discard {
                players: AffectedPlayers::EachPlayer,
                amount: crate::program::ValueExpr::Literal(1.into()),
            }],
        ),
    );
    let mut cards = vec![card(1, A, ObjectDestination::Hand, spell)];
    cards.extend(
        [(A, 10), (A, 11), (B, 20), (B, 21), (C, 30), (C, 31)]
            .into_iter()
            .map(|(owner, id)| {
                card(
                    id,
                    owner,
                    ObjectDestination::Hand,
                    Program::artifact(None, vec![]),
                )
            }),
    );
    let state = root(cards, &[20, 20, 20], None, vec![A, B, C]);
    let state = settle_three(step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    ));
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![object(&state, 10)]),
    );
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    let state = step(&state, B, GameCommand::Concede);
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(20))
            .is_none()
    );
    let state = reply(
        &state,
        DecisionReply::SelectResolution(vec![object(&state, 30)]),
    );
    let cards = state.execution().window().cards();
    assert_eq!(
        cards.get(object(&state, 10)).unwrap().zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(
        cards.get(object(&state, 11)).unwrap().zone(),
        ObjectZone::Hand(A)
    );
    assert_eq!(
        cards.get(object(&state, 30)).unwrap().zone(),
        ObjectZone::Graveyard(C)
    );
    assert_eq!(
        cards.get(object(&state, 31)).unwrap().zone(),
        ObjectZone::Hand(C)
    );
}
