//! Typed rejection inputs attached only to states reached by the ordinary policy.
//! The caller executes immutable advance, checks equality and records continuation.
use super::{
    fixture::{A, B, CaseId, Fixture, PLAINS_MANA_ABILITY, other},
    policy::{Policy, PreparedInput, lion_payment},
};
use spellwake_engine::{
    actions::TimingError,
    checks::{CheckDecision, CheckError},
    combat::{AttackDeclaration, BlockDeclaration, CombatDecision, CombatError},
    effects::EffectError,
    game::{
        DecisionId, DecisionReply, GameCommand, GameError, GameId, GameInput, GameState, RuleFrame,
    },
    mana::ManaType,
    object_identity::ObjectZone,
    opening::OpeningError,
    priority::{PriorityError, PriorityPhase, SeatId},
    stack_window::{StackWindowError, WindowPhase},
    turn::{Step, TurnError},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectionProbe {
    pub id: String,
    pub expectation_ref: String,
    pub attachment: String,
    pub input: GameInput,
    pub expected: GameError,
}
/// Prepare altered inputs without executing rules or consuming a policy entry.
pub(super) fn reached_probes(
    fixture: &Fixture,
    policy: &Policy,
    state: &GameState,
    prepared: Option<&PreparedInput>,
) -> Result<Vec<RejectionProbe>, String> {
    if matches!(fixture.case, CaseId::Selection(_)) {
        return Ok(prepared.map_or_else(Vec::new, |value| value.recipe_probes().to_vec()));
    }
    if matches!(fixture.case, CaseId::Optional(_)) {
        return super::optional::reached_probes(fixture, policy, state, prepared);
    }
    if matches!(fixture.case, CaseId::Mandatory(_)) {
        return super::mandatory::reached_probes(fixture, policy, state, prepared);
    }
    if matches!(fixture.case, CaseId::Interaction(_)) {
        return super::interaction::reached_probes(fixture, policy, state, prepared);
    }
    if matches!(fixture.case, CaseId::Teaching(_)) {
        return super::teaching::reached_probes(fixture, state, prepared);
    }
    let mut probes = vec![];
    let object = |alias: &str| fixture.current_object(state, alias);
    let mut add = |id: &str, input: GameInput, expected: GameError| {
        probes.push(RejectionProbe {
            id: format!("{}.{}", fixture.case.as_str(), id),
            expectation_ref: id.into(),
            attachment: attachment(id).into(),
            input,
            expected,
        });
    };
    if state.outcome().is_some() {
        for (id, command) in [
            ("terminal.pass", GameCommand::Pass),
            (
                "terminal.play_land",
                GameCommand::PlayLand {
                    land: object("A.P1")?,
                },
            ),
            (
                "terminal.reply_old_token",
                GameCommand::Reply {
                    decision: policy
                        .previous_decision
                        .ok_or("terminal probe lacks a real prior reply token")?,
                    choice: DecisionReply::Discard(vec![]),
                },
            ),
        ] {
            add(id, input(state, A, command), GameError::GameEnded);
        }
        return Ok(probes);
    }
    let valid = &prepared
        .ok_or("nonterminal reached probes require the one prepared input")?
        .input;
    let turn = state.turn().map(|turn| turn.position());
    let ready_a = state.execution().window().phase()
        == WindowPhase::Priority(PriorityPhase::Ready { holder: A })
        && state.pending_decision().is_none();
    let first_upkeep =
        turn.is_some_and(|turn| turn.number == 1 && turn.step == Step::Upkeep) && ready_a;
    if fixture.case == CaseId::G01 && first_upkeep {
        let mut altered = valid.clone();
        altered.basis.game = GameId(fixture.game.0 + 10000);
        add("basis.game", altered, GameError::WrongGame);
        let mut altered = valid.clone();
        altered.basis.context = fixture.probe_context()?.1;
        add("basis.context", altered, GameError::WrongContext);
        let mut altered = valid.clone();
        altered.basis.epoch += 1;
        add("basis.epoch", altered, GameError::WrongEpoch);
        let mut altered = valid.clone();
        altered.basis.revision += 1;
        add("basis.revision", altered, GameError::StaleRevision);
        let mut altered = valid.clone();
        altered.actor = SeatId(999);
        add("actor.unknown", altered, GameError::UnknownActor);
    }
    if fixture.case == CaseId::G01 && first_upkeep {
        add(
            "actor.wrong-holder",
            input(state, B, GameCommand::Pass),
            effect(EffectError::Window(StackWindowError::Priority(
                PriorityError::WrongPlayer,
            ))),
        );
        if fixture.case == CaseId::G01 {
            add(
                "timing.play_land.upkeep",
                input(
                    state,
                    A,
                    GameCommand::PlayLand {
                        land: object("A.P1")?,
                    },
                ),
                effect(EffectError::Timing(TimingError::MainPhaseRequired)),
            );
            add(
                "timing.cast_untargeted.upkeep",
                input(
                    state,
                    A,
                    GameCommand::CastUntargeted {
                        spell: object("A.L1")?,
                        payment: Some(lion_payment()),
                    },
                ),
                effect(EffectError::Timing(TimingError::MainPhaseRequired)),
            );
        }
    }
    if let Some(pending) = state.pending_decision() {
        let family = match pending.frame() {
            RuleFrame::Opening(_) if fixture.case == CaseId::G01 && state.basis().revision == 0 => {
                Some("opening")
            }
            RuleFrame::Checks(CheckDecision::CleanupDiscard(_))
                if matches!(fixture.case, CaseId::G01 | CaseId::G03)
                    && turn.is_some_and(|turn| turn.number == 2) =>
            {
                Some("cleanup")
            }
            RuleFrame::Checks(CheckDecision::GraveyardOrder(_)) if pending.chooser() == B => {
                Some("check")
            }
            RuleFrame::Combat(CombatDecision::Attackers(_))
                if fixture.case.is_trade() && turn.is_some_and(|turn| turn.number == 5) =>
            {
                Some("combat")
            }
            _ => None,
        };
        if let Some(family) = family {
            let mut altered = valid.clone();
            altered.actor = other(pending.chooser())?;
            add(
                &format!("frame.{family}.wrong-chooser"),
                altered,
                GameError::WrongChooser,
            );
            let mut altered = valid.clone();
            if let GameCommand::Reply { decision, .. } = &mut altered.command {
                *decision = DecisionId(pending.id().0 + 10000);
            } else {
                return Err("pending decision did not produce a reply recipe".into());
            }
            add(
                &format!("frame.{family}.wrong-token"),
                altered,
                GameError::WrongDecision,
            );
            let expected = if matches!(pending.frame(), RuleFrame::Opening(_)) {
                GameError::Opening(OpeningError::WrongReply)
            } else {
                effect(EffectError::WrongDecisionKind)
            };
            add(
                &format!("frame.{family}.wrong-family"),
                reply(
                    state,
                    pending.chooser(),
                    pending.id(),
                    DecisionReply::ManaColor(ManaType::White),
                ),
                expected,
            );
        }
        let old_family = match pending.frame() {
            RuleFrame::Opening(_) if fixture.case == CaseId::G01 && state.basis().revision == 1 => {
                Some("opening")
            }
            RuleFrame::Checks(CheckDecision::CleanupDiscard(_))
                if fixture.case == CaseId::G03 && turn.is_some_and(|turn| turn.number == 4) =>
            {
                Some("cleanup")
            }
            RuleFrame::Combat(CombatDecision::Attackers(_))
                if fixture.case.is_trade() && turn.is_some_and(|turn| turn.number == 7) =>
            {
                Some("combat")
            }
            RuleFrame::Checks(CheckDecision::GraveyardOrder(_)) if pending.chooser() == B => {
                Some("check")
            }
            _ => None,
        };
        if let Some(family) = old_family {
            let old = policy
                .previous_decision
                .filter(|old| *old != pending.id())
                .ok_or("old-token probe lacks a real previous same-family token")?;
            let mut altered = valid.clone();
            if let GameCommand::Reply { decision, .. } = &mut altered.command {
                *decision = old;
            }
            add(
                &format!("frame.{family}.old-token-current-basis"),
                altered,
                GameError::WrongDecision,
            );
        }
        match pending.frame() {
            RuleFrame::Combat(CombatDecision::Attackers(_))
                if fixture.case.is_trade() && turn.is_some_and(|turn| turn.number == 5) =>
            {
                let old = object("A.L1")?;
                let young = object("A.L2")?;
                for (id, rows) in [
                    (
                        "attack.young-alone",
                        vec![AttackDeclaration {
                            attacker: young,
                            defender: B,
                        }],
                    ),
                    (
                        "attack.young-with-old",
                        vec![
                            AttackDeclaration {
                                attacker: old,
                                defender: B,
                            },
                            AttackDeclaration {
                                attacker: young,
                                defender: B,
                            },
                        ],
                    ),
                ] {
                    add(
                        id,
                        reply(
                            state,
                            A,
                            pending.id(),
                            DecisionReply::DeclareAttackers(rows),
                        ),
                        effect(EffectError::Combat(CombatError::IllegalAttacker)),
                    );
                }
            }
            RuleFrame::Combat(CombatDecision::Attackers(_))
                if fixture.case.is_trade() && turn.is_some_and(|turn| turn.number == 9) =>
            {
                let first = AttackDeclaration {
                    attacker: object("A.L1")?,
                    defender: B,
                };
                add(
                    "attack.duplicate",
                    reply(
                        state,
                        A,
                        pending.id(),
                        DecisionReply::DeclareAttackers(vec![first, first]),
                    ),
                    effect(EffectError::Combat(CombatError::DuplicateAttacker)),
                );
                add(
                    "attack.defender",
                    reply(
                        state,
                        A,
                        pending.id(),
                        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                            defender: A,
                            ..first
                        }]),
                    ),
                    effect(EffectError::Combat(CombatError::IllegalDefender)),
                );
                add(
                    "attack.wrong-zone",
                    reply(
                        state,
                        A,
                        pending.id(),
                        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                            attacker: object("A.P7")?,
                            defender: B,
                        }]),
                    ),
                    effect(EffectError::Combat(CombatError::IllegalAttacker)),
                );
                add(
                    "attack.nonexistent",
                    reply(
                        state,
                        A,
                        pending.id(),
                        DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                            attacker: historical_absent(fixture, policy, state, "A.P8")?,
                            defender: B,
                        }]),
                    ),
                    effect(EffectError::Combat(CombatError::IllegalAttacker)),
                );
            }
            RuleFrame::Combat(CombatDecision::Blockers(_))
                if fixture.case.is_trade() && turn.is_some_and(|turn| turn.number == 9) =>
            {
                let first = BlockDeclaration {
                    blocker: object("B.L1")?,
                    attacker: object("A.L1")?,
                };
                add(
                    "block.duplicate",
                    reply(
                        state,
                        B,
                        pending.id(),
                        DecisionReply::DeclareBlockers(vec![
                            first,
                            BlockDeclaration {
                                attacker: object("A.L2")?,
                                ..first
                            },
                        ]),
                    ),
                    effect(EffectError::Combat(CombatError::DuplicateBlocker)),
                );
                add(
                    "block.wrong-zone",
                    reply(
                        state,
                        B,
                        pending.id(),
                        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                            blocker: object("B.P4")?,
                            ..first
                        }]),
                    ),
                    effect(EffectError::Combat(CombatError::IllegalBlocker)),
                );
                add(
                    "block.target",
                    reply(
                        state,
                        B,
                        pending.id(),
                        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                            attacker: object("A.P1")?,
                            ..first
                        }]),
                    ),
                    effect(EffectError::Combat(CombatError::IllegalBlockTarget)),
                );
                add(
                    "block.nonexistent",
                    reply(
                        state,
                        B,
                        pending.id(),
                        DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                            blocker: historical_absent(fixture, policy, state, "B.P9")?,
                            ..first
                        }]),
                    ),
                    effect(EffectError::Combat(CombatError::IllegalBlocker)),
                );
            }
            RuleFrame::Checks(CheckDecision::CleanupDiscard(_))
                if turn.is_some_and(|turn| turn.number == 2)
                    && matches!(fixture.case, CaseId::G01 | CaseId::G03) =>
            {
                let drawn = object("B.P8")?;
                let stale = *policy
                    .old_library_objects
                    .get(&fixture.card("B.P8")?)
                    .ok_or("cleanup stale probe lacks actual DrawEvent.before handle")?;
                if stale == drawn || state.execution().window().cards().get(stale).is_some() {
                    return Err(
                        "cleanup stale handle is not its real old library incarnation".into(),
                    );
                }
                for (id, rows) in [
                    ("discard.empty", vec![]),
                    ("discard.too-many", vec![object("B.P1")?, object("B.P2")?]),
                    ("discard.duplicate-wrong-cardinality", vec![drawn, drawn]),
                    ("discard.nonhand", vec![object("B.P9")?]),
                    ("discard.stale-library-incarnation", vec![stale]),
                ] {
                    add(
                        id,
                        reply(state, B, pending.id(), DecisionReply::Discard(rows)),
                        effect(EffectError::Check(CheckError::InvalidDiscard)),
                    );
                }
            }
            RuleFrame::Checks(CheckDecision::GraveyardOrder(_)) if pending.chooser() == B => {
                let first = object("B.L1")?;
                let old = object("B.P6")?;
                for (id, rows) in [
                    ("order.missing", vec![first]),
                    ("order.duplicate-correct-cardinality", vec![first, first]),
                    ("order.outside", vec![first, object("B.P4")?]),
                    ("order.old-grave-included", vec![first, old]),
                ] {
                    add(
                        id,
                        reply(state, B, pending.id(), DecisionReply::OrderGraveyard(rows)),
                        effect(EffectError::Check(CheckError::InvalidGraveyardOrder)),
                    );
                }
            }
            _ => {}
        }
    }
    if fixture.case == CaseId::G01
        && ready_a
        && turn.is_some_and(|turn| turn.number == 1 && turn.step == Step::PrecombatMain)
        && state
            .turn()
            .is_some_and(|turn| *turn.lands_played() == 1.into())
    {
        add(
            "land.second",
            input(
                state,
                A,
                GameCommand::PlayLand {
                    land: object("A.P2")?,
                },
            ),
            GameError::Turn(TurnError::LandLimitReached),
        );
        let mut altered = valid.clone();
        altered.basis = policy
            .previous_basis
            .clone()
            .ok_or("prior-basis probe lacks actual accepted input basis")?;
        add("basis.previous", altered, GameError::StaleRevision);
    }
    if fixture.case == CaseId::G01
        && ready_a
        && turn.is_some_and(|turn| turn.number == 3 && turn.step == Step::PrecombatMain)
    {
        let cards = state.execution().window().cards();
        let lion = object("A.L1")?;
        if cards
            .get(lion)
            .is_some_and(|card| card.zone() == ObjectZone::Hand(A))
            && state
                .turn()
                .is_some_and(|turn| *turn.lands_played() == 1.into())
        {
            let white = state.execution().mana(A, ManaType::White);
            if white == 0.into() {
                add(
                    "mana.empty",
                    input(
                        state,
                        A,
                        GameCommand::CastUntargeted {
                            spell: lion,
                            payment: Some(lion_payment()),
                        },
                    ),
                    effect(EffectError::InsufficientMana),
                );
            } else if white == 1.into() {
                add(
                    "mana.only-one",
                    input(
                        state,
                        A,
                        GameCommand::CastUntargeted {
                            spell: lion,
                            payment: Some(lion_payment()),
                        },
                    ),
                    effect(EffectError::InsufficientMana),
                );
                add(
                    "mana.retap",
                    input(
                        state,
                        A,
                        GameCommand::ActivateMana {
                            source: object("A.P1")?,
                            ability: PLAINS_MANA_ABILITY,
                        },
                    ),
                    effect(EffectError::Window(StackWindowError::SourceTapped)),
                );
            }
        }
    }
    Ok(probes)
}
fn input(state: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
    GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    }
}
fn reply(
    state: &GameState,
    actor: SeatId,
    decision: DecisionId,
    choice: DecisionReply,
) -> GameInput {
    input(state, actor, GameCommand::Reply { decision, choice })
}
fn effect(error: EffectError) -> GameError {
    GameError::Effect(error)
}
fn historical_absent(
    fixture: &Fixture,
    policy: &Policy,
    state: &GameState,
    alias: &str,
) -> Result<spellwake_engine::object_identity::ObjectId, String> {
    let old = *policy
        .old_library_objects
        .get(&fixture.card(alias)?)
        .ok_or("absent-object probe lacks actual old library incarnation")?;
    if state.execution().window().cards().get(old).is_some() {
        return Err("historical absent-object probe still addresses a current object".into());
    }
    Ok(old)
}
fn attachment(id: &str) -> &'static str {
    match id {
        "basis.game"
        | "basis.context"
        | "basis.epoch"
        | "basis.revision"
        | "actor.unknown"
        | "actor.wrong-holder"
        | "timing.play_land.upkeep"
        | "timing.cast_untargeted.upkeep" => "opening.complete",
        "basis.previous" | "land.second" => "land.A.1",
        "frame.opening.wrong-chooser"
        | "frame.opening.wrong-token"
        | "frame.opening.wrong-family" => "initialization",
        "frame.cleanup.wrong-chooser"
        | "frame.cleanup.wrong-token"
        | "frame.cleanup.wrong-family"
        | "discard.empty"
        | "discard.too-many"
        | "discard.duplicate-wrong-cardinality"
        | "discard.nonhand"
        | "discard.stale-library-incarnation" => "cleanup.first.before",
        "frame.combat.wrong-chooser"
        | "frame.combat.wrong-token"
        | "frame.combat.wrong-family"
        | "attack.young-alone"
        | "attack.young-with-old" => "young.live-domain",
        "frame.check.wrong-chooser"
        | "frame.check.wrong-token"
        | "frame.check.wrong-family"
        | "frame.check.old-token-current-basis"
        | "order.missing"
        | "order.duplicate-correct-cardinality"
        | "order.outside"
        | "order.old-grave-included" => "trade.owner.B",
        "frame.opening.old-token-current-basis" => "after_A_first_opening_reply_B_declare",
        "frame.cleanup.old-token-current-basis" => "G03_B2_cleanup_frame",
        "frame.combat.old-token-current-basis" => "G02_A4_attacker_frame",
        "mana.empty" => "land.A.2",
        "mana.only-one" | "mana.retap" => "mana.A.2.1",
        "attack.duplicate" | "attack.defender" | "attack.wrong-zone" | "attack.nonexistent" => {
            "G02_A5_Attackers_frame_before_prescribed_reply"
        }
        "block.duplicate" | "block.wrong-zone" | "block.nonexistent" | "block.target" => {
            "trade.before"
        }
        "terminal.pass" | "terminal.play_land" | "terminal.reply_old_token" => "terminal",
        _ => panic!("source-authored probe identity has no frozen attachment"),
    }
}
