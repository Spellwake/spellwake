use super::*;
use spellwake_engine::{
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaOutput, ManaType},
    program::{
        AffectedPlayers, PublicZoneDestination, ResolvingPlayer, ZoneActionKind,
        ZoneActionPredicate, ZoneObjectKind, ZoneReplacementAbility, ZoneReplacementAbilityId,
        ZoneReplacementChoice, ZoneReplacementSubject,
    },
};

#[test]
#[ignore = "requires retained source-qualified vocabulary"]
fn madness_projects_private_replacement_choices_and_replays_the_paid_exile_cast() {
    let mut madness = instant(
        BodyTarget::SinglePermanent,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ZoneDest::Hand,
            ),
        ))],
    );
    madness.madness = Some(ManaCost::one(ManaType::Green));
    let discard = instant(
        BodyTarget::None,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::discard(
                AffectedPlayers::One(ResolvingPlayer::Controller),
                spellwake_engine::program::ValueExpr::Literal(1.into()),
            ),
        ))],
    );
    let mut competitor = Program::artifact(None, vec![]);
    competitor.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(10),
        actions: ZoneActionPredicate::OneOf([ZoneActionKind::EffectDiscard].into()),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::Mandatory,
    });
    let land = Program::land(
        Default::default(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed([(ManaType::Green, 1.into())].into()),
        }],
    );
    let game = main_state(vec![
        card(1, A, ObjectDestination::Hand, madness),
        card(2, A, ObjectDestination::Hand, discard),
        card(
            3,
            B,
            ObjectDestination::Battlefield { controller: B },
            competitor,
        ),
        card(4, A, ObjectDestination::Battlefield { controller: A }, land),
    ]);
    let cast = bridged(&game, viewer_a(), |view| PlayerAction::CastUntargeted {
        source: view
            .issue(NativeReference::Object(object(&game, 2)))
            .unwrap(),
        payment: None,
    });
    let passed = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let mut choice = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    if matches!(
        choice.pending_decision().unwrap().frame(),
        spellwake_engine::game::RuleFrame::ResolutionSelect(_)
    ) {
        choice = bridged(&choice, viewer_a(), |access| {
            let snap = access.project(&metadata(), &Display::default()).unwrap();
            PlayerAction::Choose {
                decision: snap.decisions[0].decision.clone(),
                response: PlayerChoice::Objects {
                    objects: vec![
                        access
                            .issue(NativeReference::Object(object(&choice, 1)))
                            .unwrap(),
                    ],
                },
            }
        });
    }
    let opponent = scope(&choice, viewer_b())
        .project(&metadata(), &Display::default())
        .unwrap();
    assert!(opponent.decisions.is_empty());
    let exiled = bridged(&choice, viewer_a(), |access| {
        let snap = access.project(&metadata(), &Display::default()).unwrap();
        let decision = &snap.decisions[0];
        assert!(decision.kind == DecisionKind::ZoneReplacement);
        let PlayerDomain::Options { options, .. } = &decision.domain else {
            panic!("replacement choices")
        };
        assert_eq!(options.len(), 2);
        let madness = options
            .iter()
            .find(|option| option.label.starts_with("Apply madness to "))
            .unwrap();
        PlayerAction::Choose {
            decision: decision.decision.clone(),
            response: PlayerChoice::Option {
                option: madness.handle.clone(),
            },
        }
    });
    let passed = bridged(&exiled, viewer_a(), |_| PlayerAction::Pass {});
    let offer = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    let cast = bridged(&offer, viewer_a(), |access| {
        let snap = access.project(&metadata(), &Display::default()).unwrap();
        let decision = &snap.decisions[0];
        let PlayerDomain::MadnessCast {
            generic, required, ..
        } = &decision.domain
        else {
            panic!("madness price")
        };
        assert!(!generic.as_rules_integer().is_positive());
        assert!(required.len() == 1 && required[0].color == ManaColor::Green);
        let source = object(&offer, 4);
        let producer = PlayerCastManaActivation {
            recipient_seat_id: None,
            cost_objects: Vec::new(),
            source: access.issue(NativeReference::Object(source)).unwrap(),
            ability: access
                .issue(NativeReference::ManaAbility {
                    source,
                    ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
                })
                .unwrap(),
            color: None,
            payment: None,
            bundle: None,
        };
        PlayerAction::Choose {
            decision: decision.decision.clone(),
            response: PlayerChoice::MadnessCast {
                face: None,
                adventure: false,
                chosen_x: None,
                target: Some(PlayerTarget::Object {
                    handle: access
                        .issue(NativeReference::Object(object(&offer, 3)))
                        .unwrap(),
                }),
                payment: Some(PlayerPayment {
                    from_lots: Default::default(),
                    exile_cast_type: None,
                    alternative_cost_source: None,
                    graveyard_permission: None,
                    use_granted_flashback: false,
                    restricted_mana: Vec::new(),
                    spend: vec![ManaSpend {
                        color: ManaColor::Green,
                        amount: RulesCount::try_from("1".to_owned()).unwrap(),
                    }],
                }),
                additional: vec![],
                resources: vec![],
                mana_abilities: vec![producer],
            },
        }
    });
    let passed = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let done = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    let cards = done.execution().window().cards();
    assert_eq!(
        cards.get(object(&done, 1)).unwrap().zone(),
        ObjectZone::Graveyard(A)
    );
    assert_eq!(
        cards.get(object(&done, 3)).unwrap().zone(),
        ObjectZone::Hand(B)
    );
    assert_eq!(cards.get(object(&done, 4)).unwrap().tapped(), Some(true));
}
