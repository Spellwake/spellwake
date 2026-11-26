//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! Actual root combat, assignments and shared damage. Reads no expectations.
use serde_json::{Value, json};
use spellwake_contracts::RulesInteger;
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::{
    characteristics::{PowerToughness, ResolvedPartSpec},
    checks::{CheckBoundary, CheckDecision},
    combat::{
        AssignmentOrigin, AttackDeclaration, BlockDeclaration, BlockStatus, CombatDamageAssignment,
        CombatDecision, CombatDeclarationRecord, CombatRules, DeclarationOrigin,
    },
    damage::{DamageRecipient, DamageSource},
    draw::GameOutcome,
    effects::{DamageDecision, DecisionReply, Instruction, Program, SeedCard},
    game::{GameBasis, GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    stack_window::{StackKind, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use spellwake_reference_tools::check_trace::{
    check_decision_view, check_game_object_view, check_journal_view,
};
use spellwake_reference_tools::damage_trace::{
    damage_application_view, damage_journal_view, damage_origin_view,
};
use std::collections::{BTreeMap, BTreeSet};
const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);
const C: SeatId = SeatId(99);
const D: SeatId = SeatId(7);
type TraceResult<T> = Result<T, String>;
fn basis_view(basis: &GameBasis) -> Value {
    json!({"game":basis.game.0,"context":spellwake_reference_tools::projection::context(&basis.context),
        "epoch":basis.epoch,"revision":basis.revision})
}
fn seat(p: SeatId) -> &'static str {
    match p {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        _ => panic!("unknown fixture seat"),
    }
}
fn phase(step: Step) -> &'static str {
    match step {
        Step::Untap => "UNTAP",
        Step::Upkeep => "UPKEEP",
        Step::Draw => "DRAW",
        Step::PrecombatMain => "MAIN1",
        Step::BeginningCombat => "COMBAT_BEGIN",
        Step::DeclareAttackers => "COMBAT_DECLARE_ATTACKERS",
        Step::DeclareBlockers => "COMBAT_DECLARE_BLOCKERS",
        Step::CombatDamage => "COMBAT_DAMAGE",
        Step::EndCombat => "COMBAT_END",
        Step::PostcombatMain => "MAIN2",
        Step::End => "END_OF_TURN",
        Step::Cleanup => "CLEANUP",
    }
}
fn creature(id: u64, owner: SeatId, wurm: bool) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Battlefield { controller: owner },
        program: Program::creature(
            None,
            if wurm { 6.into() } else { 3.into() },
            if wurm { 4.into() } else { 3.into() },
            std::collections::BTreeSet::new(),
        ),
    }
}
fn land(id: u64, owner: SeatId, color: ManaType, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::land(
            std::collections::BTreeSet::new(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed(BTreeMap::from([(color, 1.into())])),
            }],
        ),
    }
}
struct Fixture {
    state: GameState,
    primary: BTreeMap<String, CardId>,
    names: BTreeMap<String, String>,
    rows: Vec<Value>,
    commands: Vec<Value>,
    choices: Vec<Value>,
    declarations: Vec<Value>,
    payments: Vec<Value>,
    funding: Vec<Value>,
    native_only: Vec<Value>,
    journals: Vec<Value>,
    check_journals: Vec<Value>,
    current_checks: Value,
    assignments: Vec<Value>,
    applied: Vec<Value>,
    preventions: Vec<Value>,
    starting_life: Value,
}
impl Fixture {
    fn new(case: &str) -> TraceResult<Self> {
        let order = match case {
            "D10" => vec![A, C, B],
            "D11" => vec![A, D, C, B],
            _ => vec![A, B],
        };
        let mut primary = BTreeMap::new();
        let mut names = BTreeMap::new();
        let mut cards = vec![];
        let mut add = |alias: &str, name: &str, card: SeedCard| {
            primary.insert(alias.to_owned(), card.card);
            names.insert(alias.to_owned(), name.to_owned());
            cards.push(card);
        };
        add("a1", "Craw Wurm", creature(1, A, true));
        let wurm = matches!(case, "D02" | "D08");
        add(
            "b1",
            if wurm { "Craw Wurm" } else { "Hill Giant" },
            creature(10, B, wurm),
        );
        if matches!(case, "D03" | "D04" | "D13" | "D14" | "N01" | "N02") {
            add("b2", "Hill Giant", creature(11, B, false));
        }
        if matches!(case, "D10" | "D11") {
            add("a2", "Hill Giant", creature(2, A, false));
            add("c1", "Hill Giant", creature(20, C, false));
        }
        if case == "D11" {
            add("d1", "Hill Giant", creature(30, D, false));
        }
        let response = match case {
            "D05" | "D06" | "D07" => Some((
                "Unsummon",
                B,
                ManaType::Blue,
                Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    spellwake_engine::toolbox::ZoneDest::Hand,
                ))),
            )),
            "D08" => Some((
                "Giant Growth",
                A,
                ManaType::Green,
                Instruction::ApplyContinuous(Box::new(
                    spellwake_engine::toolbox::ApplyContinuousSpec::resolved_parts(
                        spellwake_engine::toolbox::ObjectQuery::Targets,
                        spellwake_engine::characteristics::ResolvedPartsSpec(vec![
                            ResolvedPartSpec::ModifyPT(PowerToughness {
                                power: 3.into(),
                                toughness: 3.into(),
                            }),
                        ]),
                        spellwake_engine::characteristics::ResolvedDuration::NoStatedDuration,
                    ),
                )),
            )),
            "D12" | "D13" | "D14" | "N01" | "N02" => Some((
                "Healing Salve",
                A,
                ManaType::White,
                Instruction::CreatePreventionShield { amount: 3.into() },
            )),
            _ => None,
        };
        if let Some((name, owner, color, instruction)) = response {
            add(
                "response",
                name,
                SeedCard {
                    card: CardId(90),
                    owner,
                    destination: ObjectDestination::Hand,
                    program: Program::instant(
                        ManaCost::one(color),
                        ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
                    ),
                },
            );
        }
        for (i, &owner) in order.iter().enumerate() {
            for n in 0..10 {
                cards.push(land(
                    1000 + i as u64 * 100 + n,
                    owner,
                    ManaType::Blue,
                    ObjectDestination::LibraryBottom,
                ));
            }
        }
        for (id, owner, color) in [
            (500, B, ManaType::Blue),
            (501, A, ManaType::Green),
            (502, A, ManaType::White),
        ] {
            cards.push(land(
                id,
                owner,
                color,
                ObjectDestination::Battlefield { controller: owner },
            ));
        }
        let mut seed = {
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        spellwake_engine::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                order,
                A,
                cards,
                initial_battlefield_order,
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                spellwake_reference_tools::creature_types::selected_vocabulary()?,
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        };
        if case == "D09" {
            seed.life.iter_mut().find(|l| l.player == B).unwrap().total = 6.into();
        }
        let starting_life = json!(
            seed.life
                .iter()
                .map(|l| (seat(l.player), l.total.clone()))
                .collect::<BTreeMap<_, _>>()
        );
        let initialized = spellwake_reference_tools::current_capture::initialize_scheduled(
            GameId(1802),
            GameContext::new(
                "shared-damage".into(),
                spellwake_reference_tools::creature_types::selected_vocabulary()?
                    .rules_revision()
                    .to_owned(),
                "authored".into(),
                "literal".into(),
                "unrestricted-FFA".into(),
                spellwake_reference_tools::creature_types::selected_vocabulary()?.id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .map_err(|e| format!("{e:?}"))?,
            seed,
            TurnStart::default(),
        )
        .map_err(|e| format!("{e:?}"))?;
        let current_checks = serde_json::to_value(check_journal_view(
            initialized.check_journal(),
            initialized.state().execution().window().cards(),
            |id| {
                primary
                    .iter()
                    .find(|(_, card)| **card == id)
                    .map(|(alias, _)| alias.clone())
                    .unwrap_or_else(|| format!("native-support-{}", id.0))
            },
        )?)
        .map_err(|error| error.to_string())?;
        let state = initialized.into_state();
        Ok(Self {
            state,
            primary,
            names,
            starting_life,
            rows: vec![],
            commands: vec![],
            choices: vec![],
            declarations: vec![],
            payments: vec![],
            funding: vec![],
            native_only: vec![],
            journals: vec![],
            check_journals: vec![],
            current_checks,
            assignments: vec![],
            applied: vec![],
            preventions: vec![],
        })
    }
    fn label(&self, id: CardId) -> String {
        self.primary
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| format!("native-support-{}", id.0))
    }
    fn object(&self, label: &str) -> ObjectId {
        self.state
            .execution()
            .window()
            .cards()
            .current_identity(self.primary[label])
            .expect("fixture card exists")
    }
    fn alias(&self, id: ObjectId) -> String {
        let card = self
            .state
            .execution()
            .window()
            .cards()
            .get(id)
            .expect("observed live combat incarnation");
        self.label(card.card_id().expect("source-authored card-only trace"))
    }
    fn holder(&self) -> TraceResult<SeatId> {
        match self.state.execution().window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Ok(holder),
            other => Err(format!("expected priority, got {other:?}")),
        }
    }
    fn step(&self) -> Step {
        self.state
            .turn()
            .expect("scheduled fixture")
            .position()
            .step
    }
    fn protocol(&self) -> Value {
        let execution = self.state.execution();
        let priority = match execution.window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Some(seat(holder)),
            _ => None,
        };
        json!({"basis":basis_view(self.state.basis()),"basis_revision": self.state.basis().revision,
            "executor_revision": execution.revision(),
            "window_phase":format!("{:?}",execution.window().phase()),
            "stack":execution.window().entries().iter().map(|entry|json!({"entry":format!("{:?}",entry.id()),"controller":seat(entry.controller()),"kind":match entry.kind(){StackKind::CardSpell(object)=>json!({"kind":"card_spell","object":format!("{object:?}")}),StackKind::Speed=>json!({"kind":"Speed"}),StackKind::Monarch=>json!({"kind":"Monarch"}),StackKind::DelayedCopy{original}=>json!({"kind":"DelayedCopy","original":format!("{original:?}")}),StackKind::Emblem{emblem}=>json!({"kind":"Emblem","id":emblem.sequence()}),
            StackKind::SpellCopy{copied_from,owner}=>json!({"kind":"spell_copy","copied_from":format!("{copied_from:?}"),"owner":seat(*owner)}),StackKind::ActivatedAbility{source}=>json!({"kind":"activated_ability","source_object":format!("{:?}",source.id()),"physical_card":source.card_id().expect("source-authored card-only trace").0}),StackKind::TriggeredAbility{source}=>json!({"kind":"triggered_ability","source_object":format!("{:?}",source.id()),"physical_card":source.card_id().expect("source-authored card-only trace").0})}})).collect::<Vec<_>>(),
            "active": seat(execution.window().active_player()), "priority": priority,
            "step": phase(self.step()), "stage": format!("{:?}", self.state.turn().unwrap().stage()),
            "combat_context_present": execution.combat().is_some(),
            "had_attackers": execution.combat().map(|c| c.had_attackers()),
            "pending_chooser": self.state.pending_decision().map(|p| seat(p.chooser())),
            "pending_decision": self.state.pending_decision().map(|p| format!("{:?}", p.id()))})
    }
    fn act(&mut self, actor: SeatId, command: GameCommand) -> TraceResult<()> {
        let command_text = format!("{command:?}");
        let before_revision = self.state.basis().revision;
        let before = self.protocol();
        let state_before = self.snapshot("command-before", "native-only-command")?;
        let current_frame = self.decision_view();
        let object_state=self.state.execution().window().cards().iter().map(|card|json!({"object":format!("{:?}",card.id()),"card":card.card_id().expect("source-authored card-only trace").0,"alias":self.label(card.card_id().expect("source-authored card-only trace")),"owner":seat(card.owner()),"controller":card.controller().map(seat),"zone":format!("{:?}",card.zone()),"tapped":card.tapped(),"epoch":card.control_epoch().map(|e|format!("{e:?}")),"continuous":card.has_continuous_control_since_turn_start()})).collect::<Vec<_>>();
        let objects = self
            .state
            .execution()
            .window()
            .cards()
            .iter()
            .map(|card| {
                (
                    format!("{:?}", card.id()),
                    self.label(card.card_id().expect("source-authored card-only trace")),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let choice = match &command {
            GameCommand::Reply {
                choice: DecisionReply::DeclareAttackers(selected),
                ..
            } => Some(
                json!({"kind":"attackers","actor":seat(actor),"selected":selected.iter().map(|d|json!({"attacker":self.alias(d.attacker),"defender":seat(d.defender)})).collect::<Vec<_>>()}),
            ),
            GameCommand::Reply {
                choice: DecisionReply::DeclareBlockers(selected),
                ..
            } => Some(
                json!({"kind":"blockers","actor":seat(actor),"selected":selected.iter().map(|d|json!({"blocker":self.alias(d.blocker),"attacker":self.alias(d.attacker)})).collect::<Vec<_>>()}),
            ),
            GameCommand::Reply {
                choice: DecisionReply::AssignCombatDamage(rows),
                ..
            } => Some(
                json!({"kind":"assignments","actor":seat(actor),"selected":rows.iter().map(|r|json!({"source":self.alias(r.source),"target":self.recipient(r.target),"amount":r.amount})).collect::<Vec<_>>()}),
            ),
            GameCommand::Reply {
                choice: DecisionReply::AllocatePrevention(rows),
                ..
            } => Some(
                json!({"kind":"prevention","actor":seat(actor),"selected":rows.iter().map(|r|json!({"attempt":format!("{:?}",r.attempt),"amount":r.amount})).collect::<Vec<_>>()}),
            ),
            GameCommand::Reply {
                choice: DecisionReply::OrderGraveyard(selected),
                ..
            } => Some(
                json!({"kind":"graveyard_order","actor":seat(actor),"selected":selected.iter().map(|id|json!({"object":format!("{id:?}"),"alias":self.alias(*id)})).collect::<Vec<_>>()}),
            ),
            _ => None,
        };
        let intent = match &command {
            GameCommand::Pass => json!({"kind":"Pass"}),
            GameCommand::ActivateMana { source, ability } => {
                json!({"kind":"ActivateMana","source":format!("{source:?}"),"ability":spellwake_reference_tools::mana_ref::view(*ability)})
            }
            GameCommand::CastUntargeted { spell, .. } => {
                json!({"kind":"CastUntargeted","spell":format!("{spell:?}")})
            }
            GameCommand::Cast {
                spell,
                target: TargetRef::Object(target),
            } => json!({"kind":"Cast","spell":format!("{spell:?}"),"target":format!("{target:?}")}),
            GameCommand::Reply { decision, .. } => {
                json!({"kind":"Reply","decision":format!("{decision:?}"),"choice":choice})
            }
            _ => return Err("fixture used an unrecorded command variant".into()),
        };
        let cast_alias = match &command {
            GameCommand::CastUntargeted { spell, .. } | GameCommand::Cast { spell, .. } => {
                Some(self.alias(*spell))
            }
            _ => None,
        };
        let target_alias = match &command {
            GameCommand::Cast {
                target: TargetRef::Object(target),
                ..
            } => Some(self.alias(*target)),
            _ => None,
        };
        let candidate = spellwake_reference_tools::current_capture::advance(
            &self.state,
            GameInput {
                basis: self.state.basis().clone(),
                actor,
                command,
            },
        )
        .map_err(|e| format!("native command {command_text}: {e:?}"))?;
        if let Some(choice) = choice {
            self.choices.push(choice);
        }
        for record in candidate.combat_declarations() {
            match record {
                CombatDeclarationRecord::Attackers{player,origin,attackers,taps}=>self.declarations.push(json!({"command_revision":before_revision,"kind":"attackers","actor":seat(*player),"origin":match origin{DeclarationOrigin::PlayerReply=>"PlayerReply",DeclarationOrigin::ForcedEmpty=>"ForcedEmpty"},"selected":attackers.iter().map(|d|json!({"attacker":self.alias(d.attacker),"defender":seat(d.defender)})).collect::<Vec<_>>(),"taps":taps.iter().map(|t|json!({"card":self.label(t.before.card_id().expect("source-authored card-only trace")),"before":t.before.tapped(),"after":t.after.tapped(),"same_incarnation":t.before.id()==t.after.id(),"same_control_epoch":t.before.control_epoch()==t.after.control_epoch()})).collect::<Vec<_>>()})),
                CombatDeclarationRecord::Blockers{player,origin,blockers}=>self.declarations.push(json!({"command_revision":before_revision,"kind":"blockers","actor":seat(*player),"origin":match origin{DeclarationOrigin::PlayerReply=>"PlayerReply",DeclarationOrigin::ForcedEmpty=>"ForcedEmpty"},"selected":blockers.iter().map(|d|json!({"blocker":self.alias(d.blocker),"attacker":self.alias(d.attacker)})).collect::<Vec<_>>()})),
            }
        }
        if let Some(payment) = candidate.mana_payment() {
            let required = payment
                .cost
                .required
                .iter()
                .map(|(t, n)| (format!("{t:?}"), n.clone()))
                .collect::<BTreeMap<_, _>>();
            let spend = payment
                .spend
                .iter()
                .map(|s| (format!("{:?}", s.mana_type), s.amount.clone()))
                .collect::<BTreeMap<_, _>>();
            self.payments.push(json!({"card":cast_alias,"actor":seat(payment.payer),"target":target_alias,"cost":{"generic":payment.cost.generic,"required":required},"spend":spend,"basis_revision":before_revision}));
        }
        if let Some(production) = candidate.mana_production() {
            self.funding.push(json!({"command_revision":before_revision,"actor":seat(production.controller),"source":format!("{:?}",production.source),"produced":production.produced.iter().map(|(t,n)|(format!("{t:?}"),n.clone())).collect::<BTreeMap<_,_>>() }));
        }
        let damage_journal = damage_journal_view(candidate.damage_journal());
        let check_journal = serde_json::to_value(check_journal_view(
            candidate.check_journal(),
            self.state.execution().window().cards(),
            |id| self.label(id),
        )?)
        .map_err(|error| error.to_string())?;
        self.check_journals
            .push(json!({"command_revision":before_revision,"journal":check_journal}));
        let journal = candidate.damage_journal();
        self.journals.push(
            json!({"command_revision":before_revision,"journal":damage_journal_view(journal)}),
        );
        for record in candidate.combat_assignments() {
            self.assignments.push(json!({"command_revision":before_revision,"actor":seat(record.player),"origin":match record.origin{AssignmentOrigin::Forced=>"Forced",AssignmentOrigin::PlayerReply=>"PlayerReply"},"selected":record.assignments.iter().map(|r|json!({"source":self.alias(r.source),"source_object":format!("{:?}",r.source),"target":self.recipient(r.target),"amount":r.amount})).collect::<Vec<_>>()}));
        }
        for fact in &journal.applied {
            let DamageSource::Object { object, .. } = &fact.source else {
                return Err("combat fixture has a nonphysical source".into());
            };
            self.applied.push(json!({"source":self.label(object.card_id().expect("source-authored card-only trace")),"target":self.recipient(fact.target),"amount":fact.amount,"kind":"Combat"}));
        }
        for fact in &journal.preventions {
            let active = self.state.execution().pending_damage();
            let source_for = |id| {
                journal
                    .attempts
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| &a.source)
                    .or_else(|| {
                        active
                            .and_then(|b| b.component(id))
                            .map(|c| &c.attempt().source)
                    })
            };
            let mut allocations = vec![];
            for r in &fact.allocations {
                let Some(DamageSource::Object { object, .. }) = source_for(r.attempt) else {
                    return Err("missing prevention source lineage".into());
                };
                allocations.push(json!({"source":self.label(object.card_id().expect("source-authored card-only trace")),"amount":r.amount}));
            }
            self.preventions.push(json!({"command_revision":before_revision,"target":self.recipient(fact.target),"allocations":allocations,"total":fact.amount,"remainder":fact.shield_remaining}));
        }
        self.current_checks = check_journal.clone();
        self.state = candidate.into_state();
        self.commands.push(spellwake_reference_tools::capture_points::mark_value("Fixture::commands actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),&spellwake_reference_tools::packet::AliasContext::default(),json!({"actor":seat(actor),"command":command_text,"intent":intent,"objects_before":objects,"object_state_before":object_state,"before":before,"decision_before":current_frame,"state_before":state_before,"state_after":self.snapshot("command-after","native-only-command")?,"decision_after":self.decision_view(),"after":self.protocol(),"basis_revision":before_revision,"next_revision":self.state.basis().revision,"step_after":phase(self.step()),"damage_journal":damage_journal,"check_journal":check_journal})).expect("collect actual scoped native point"));
        Ok(())
    }
    fn pass(&mut self) -> TraceResult<()> {
        self.act(self.holder()?, GameCommand::Pass)
    }
    fn round(&mut self) -> TraceResult<()> {
        for _ in 0..self.state.execution().window().participants().len() {
            self.pass()?;
        }
        Ok(())
    }
    fn go(&mut self, step: Step) -> TraceResult<()> {
        for _ in 0..24 {
            if self.step() == step {
                return Ok(());
            }
            self.round()?;
        }
        Err(format!("bounded fixture failed to reach {step:?}"))
    }
    fn attack(&mut self, selection: &[(&str, SeatId)]) -> TraceResult<()> {
        let pending = self
            .state
            .pending_decision()
            .ok_or("missing attacker decision")?;
        if !matches!(
            pending.frame(),
            RuleFrame::Combat(CombatDecision::Attackers(_))
        ) {
            return Err("wrong attacker frame".into());
        }
        let decision = pending.id();
        let actor = pending.chooser();
        let selected = selection
            .iter()
            .map(|(label, defender)| AttackDeclaration {
                attacker: self.object(label),
                defender: *defender,
                defending_planeswalker: None,
            })
            .collect();
        self.act(
            actor,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::DeclareAttackers(selected),
            },
        )
    }
    fn block(&mut self, selection: &[(&str, &str)]) -> TraceResult<()> {
        let pending = self
            .state
            .pending_decision()
            .ok_or("missing blocker decision")?;
        if !matches!(
            pending.frame(),
            RuleFrame::Combat(CombatDecision::Blockers(_))
        ) {
            return Err("wrong blocker frame".into());
        }
        let decision = pending.id();
        let actor = pending.chooser();
        let selected = selection
            .iter()
            .map(|(blocker, attacker)| BlockDeclaration {
                blocker: self.object(blocker),
                attacker: self.object(attacker),
            })
            .collect();
        self.act(
            actor,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::DeclareBlockers(selected),
            },
        )
    }
    fn fund(&mut self, actor: SeatId, ids: &[u64]) -> TraceResult<()> {
        for &id in ids {
            let source = self
                .state
                .execution()
                .window()
                .cards()
                .current_identity(CardId(id))
                .ok_or("missing native mana support")?;
            self.act(
                actor,
                GameCommand::ActivateMana {
                    source,
                    ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
                },
            )?;
        }
        Ok(())
    }
    fn graveyards(&self) -> Value {
        let cards = self.state.execution().window().cards();
        json!(
            self.state
                .execution()
                .window()
                .participants()
                .iter()
                .map(|&player| {
                    let ordered = cards
                        .graveyard(player)
                        .iter()
                        .map(|id| {
                            check_game_object_view(
                                cards.get(*id).expect("ordered current graveyard object"),
                                |card| self.label(card),
                            )
                        })
                        .collect::<Vec<_>>();
                    (seat(player), ordered)
                })
                .collect::<BTreeMap<_, _>>()
        )
    }
    fn snapshot(&self, id: &str, scope: &str) -> TraceResult<Value> {
        let execution = self.state.execution();
        let evaluated = execution
            .evaluated_characteristics()
            .map_err(|error| format!("characteristic observation rejected: {error:?}"))?;
        let combat = execution.combat();
        let mut cards = serde_json::Map::new();
        for (alias, card) in &self.primary {
            let registry = self.state.execution().window().cards();
            let current = registry
                .get(registry.current_identity(*card).unwrap())
                .unwrap();
            let a = combat.and_then(|c| c.attacker(current.id()));
            let b = combat.and_then(|c| c.blocker(current.id()));
            let mut blockers = combat
                .map(|c| {
                    c.blockers()
                        .filter(|b| b.attacker() == Some(current.id()))
                        .map(|b| self.alias(b.combatant().object()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            blockers.sort();
            let linked = b
                .and_then(|b| b.attacker())
                .map(|a| vec![self.alias(a)])
                .unwrap_or_default();
            let zone = match current.zone() {
                ObjectZone::Battlefield => "Battlefield",
                ObjectZone::Hand(_) => "Hand",
                ObjectZone::Stack => "Stack",
                ObjectZone::Graveyard(_) => "Graveyard",
                other => panic!("unexpected fixture zone {other:?}"),
            };
            let characteristics = evaluated
                .get(current.id())
                .ok_or_else(|| format!("characteristic observation omitted {:?}", current.id()))?;
            let creature = characteristics
                .power_toughness
                .as_ref()
                .map(|value| &value.derived);
            cards.insert(alias.clone(),json!({"name":self.names[alias],"power":creature.map(|c|c.power.clone()),"toughness":creature.map(|c|c.toughness.clone()),"damage":execution.battlefield_marked_damage(current.id()),"owner":seat(current.owner()),"controller":current.controller().map(seat),"zone":zone,"tapped":current.tapped(),"continuous":current.has_continuous_control_since_turn_start(),"attacking":a.is_some(),"blocking":b.is_some(),"defender":a.map(|a|seat(a.defender())),"block_status":a.map(|a|match a.block_status(){BlockStatus::NotDeclared=>"NotDeclared",BlockStatus::Unblocked=>"Unblocked",BlockStatus::Blocked=>"Blocked"}),"attackers_blocked":linked,"blockers":blockers}));
        }
        let mut attackers = combat
            .map(|c| {
                c.attackers()
                    .map(|a| self.alias(a.combatant().object()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        attackers.sort();
        let mut blockers = combat
            .map(|c| {
                c.blockers()
                    .map(|b| self.alias(b.combatant().object()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        blockers.sort();
        let priority = match self.state.execution().window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Some(seat(holder)),
            _ => None,
        };
        Ok(
            json!({"id":id,"comparison_scope":scope,"protocol":"actual-native-root","turn":self.state.turn().unwrap().position().number,"life":self.state.execution().window().participants().iter().map(|&p|(seat(p),self.state.execution().life_total(p).unwrap().clone())).collect::<BTreeMap<_,_>>(),"game_over":self.state.outcome().is_some(),"live_shield_capacity":self.state.execution().prevention_shields().fold(RulesInteger::from(0),|total,s|total.plus(&s.remaining)),"step":phase(self.step()),"active":seat(self.state.execution().window().active_player()),"priority":priority,"stack_size":self.state.execution().window().entries().len(),"cards":cards,"graveyards_top_to_bottom":self.graveyards(),"check_journal":self.current_checks,"combat":{"attackers":attackers,"blockers":blockers},"native":{"basis":basis_view(self.state.basis()),"basis_revision":self.state.basis().revision,"executor_revision":self.state.execution().revision(),"combat_context_present":combat.is_some(),"had_attackers":combat.map(|c|c.had_attackers()),"stage":format!("{:?}",self.state.turn().unwrap().stage()),"pending_chooser":self.state.pending_decision().map(|p|seat(p.chooser())),"decision":self.decision_view()}}),
        )
    }
    fn row(&mut self, id: &str) -> TraceResult<()> {
        let scope = if id.starts_with("N01-") || id.starts_with("N02-") {
            "native-root-only"
        } else if matches!(
            id,
            "D05-damage-priority"
                | "D06-damage-priority"
                | "D07-blocker-priority"
                | "D07-damage-priority"
        ) {
            "native-root-only-protocol"
        } else {
            "common"
        };
        self.rows.push(
            spellwake_reference_tools::capture_points::mark_value(
                "Fixture::rows actual writer",
                spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),
                &spellwake_reference_tools::packet::AliasContext::default(),
                self.snapshot(id, scope)?,
            )
            .expect("collect actual scoped native point"),
        );
        Ok(())
    }

    fn recipient(&self, target: DamageRecipient) -> String {
        match target {
            DamageRecipient::Creature(id)
            | DamageRecipient::Planeswalker(id)
            | DamageRecipient::Battle(id) => self.alias(id),
            DamageRecipient::Player(p) => seat(p).to_owned(),
        }
    }
    fn decision_view(&self) -> Value {
        let Some(p) = self.state.pending_decision() else {
            return Value::Null;
        };
        let frame = match p.frame() {
            RuleFrame::Checks(frame) => json!(
                check_decision_view(frame, self.state.execution().window().cards(), |id| self
                    .label(id),)
                .expect("exact current check domain")
            ),
            RuleFrame::Combat(CombatDecision::DamageAssignments(f)) => {
                json!({"kind":"assignments","revision":f.revision(),"position":{"turn":f.position().number,"step":phase(f.position().step),"additional":f.position().additional_damage},"domains":f.domains().iter().map(|d|json!({"source":self.alias(d.source()),"source_object":format!("{:?}",d.source()),"power":d.power(),"recipients":d.recipients().iter().map(|&r|self.recipient(r)).collect::<Vec<_>>()})).collect::<Vec<_>>()})
            }
            RuleFrame::Damage(DamageDecision::Allocation(f)) => {
                json!({"kind":"prevention","revision":f.revision(),"batch":format!("{:?}",f.batch()),"stage":f.stage(),"origin":damage_origin_view(*f.origin()),"application":damage_application_view(f.application()),"capacity":f.capacity(),"required":f.required(),"eligible":f.eligible().iter().map(|d|{let DamageSource::Object{object,..}=d.source()else{panic!("physical source")};json!({"attempt":format!("{:?}",d.attempt()),"source":self.label(object.card_id().expect("source-authored card-only trace")),"source_object":format!("{:?}",object.id()),"amount":d.amount()})}).collect::<Vec<_>>()})
            }
            RuleFrame::Combat(CombatDecision::Attackers(_)) => json!({"kind":"attackers"}),
            RuleFrame::Combat(CombatDecision::Blockers(_)) => json!({"kind":"blockers"}),
            other => panic!("unexpected fixture frame {other:?}"),
        };
        json!({"id":format!("{:?}",p.id()),"chooser":seat(p.chooser()),"basis":basis_view(p.basis()),"basis_revision":p.basis().revision,"frame":frame})
    }
    fn damage_choices(&mut self, case: &str) -> TraceResult<()> {
        let needs_graveyard_order = matches!(case, "D13" | "D14" | "N01" | "N02");
        let mut graveyard_ordered = false;
        for _ in 0..8 {
            let Some(p) = self.state.pending_decision() else {
                if needs_graveyard_order != graveyard_ordered {
                    return Err(format!("missing actual graveyard arrangement for {case}"));
                }
                if graveyard_ordered {
                    let cards = self.state.execution().window().cards();
                    let actual = cards
                        .graveyard(B)
                        .iter()
                        .map(|id| {
                            cards
                                .get(*id)
                                .expect("actual graveyard member")
                                .card_id()
                                .expect("source-authored card-only trace")
                        })
                        .collect::<Vec<_>>();
                    if actual != [self.primary["b1"], self.primary["b2"]] {
                        return Err(format!(
                            "actual graveyard order differs from accepted [b1,b2] for {case}"
                        ));
                    }
                }
                return Ok(());
            };
            let decision = p.id();
            let actor = p.chooser();
            let choice = match p.frame() {
                RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) => {
                    if !needs_graveyard_order
                        || graveyard_ordered
                        || actor != B
                        || frame.chooser() != B
                        || frame.boundary() != CheckBoundary::Priority
                        || frame.event().pass() != 0
                    {
                        return Err(format!("unexpected graveyard arrangement for {case}"));
                    }
                    let selected = vec![self.object("b1"), self.object("b2")];
                    if frame.domain().len() != 2
                        || frame.domain().len() != 2
                        || frame.domain().iter().copied().collect::<BTreeSet<_>>()
                            != selected.iter().copied().collect()
                    {
                        return Err(format!("wrong B entrant domain for {case}"));
                    }
                    for id in &selected {
                        let current = self
                            .state
                            .execution()
                            .window()
                            .cards()
                            .get(*id)
                            .ok_or("missing pending graveyard entrant")?;
                        if current.owner() != B || current.zone() != ObjectZone::Battlefield {
                            return Err(
                                "graveyard arrangement exposed a wrong-owner or moved entrant"
                                    .into(),
                            );
                        }
                    }
                    self.native_only.push(spellwake_reference_tools::capture_points::mark_value("Fixture::native_only actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),&spellwake_reference_tools::packet::AliasContext::default(),json!({"kind":"graveyard-order-pending","decision":self.decision_view(),"snapshot":self.snapshot("graveyard-order-pending","native-root-only")?})).expect("collect actual scoped native point"));
                    graveyard_ordered = true;
                    // The reply is the desired final top-to-bottom segment.
                    // Physical insertion direction belongs to the native executor.
                    DecisionReply::OrderGraveyard(selected)
                }
                RuleFrame::Combat(CombatDecision::DamageAssignments(f)) => {
                    let mut rows = vec![];
                    for domain in f.domains() {
                        if domain.recipients().len() == 1 {
                            rows.push(CombatDamageAssignment {
                                source: domain.source(),
                                target: domain.recipients()[0],
                                amount: domain.power().clone(),
                            });
                            continue;
                        }
                        let amounts = match case {
                            "D03" => vec![("b1", 1), ("b2", 5)],
                            "D04" => vec![("b1", 6)],
                            _ => vec![("b1", 3), ("b2", 3)],
                        };
                        for (label, amount) in amounts {
                            rows.push(CombatDamageAssignment {
                                source: domain.source(),
                                target: DamageRecipient::Creature(self.object(label)),
                                amount: amount.into(),
                            });
                        }
                    }
                    DecisionReply::AssignCombatDamage(rows)
                }
                RuleFrame::Damage(DamageDecision::Allocation(f)) => {
                    self.native_only.push(spellwake_reference_tools::capture_points::mark_value("Fixture::native_only actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),&spellwake_reference_tools::packet::AliasContext::default(),json!({"kind":"allocation-pending","decision":self.decision_view(),"snapshot":self.snapshot("allocation-pending","native-root-only")?,"shield_capacity":f.capacity()})).expect("collect actual scoped native point"));
                    let source_amounts = match case {
                        "N01" => vec![("b1", 1), ("b2", 2)],
                        "N02" => vec![("b1", 2), ("b2", 1)],
                        "D14" => vec![("b2", 3)],
                        _ => vec![("b1", 3)],
                    };
                    let mut rows = vec![];
                    for (label, amount) in source_amounts {
                        let domain=f.eligible().iter().find(|d|matches!(d.source(),DamageSource::Object{object,..}if self.label(object.card_id().expect("source-authored card-only trace"))==label)).ok_or("missing selected source")?;
                        rows.push(spellwake_engine::damage::DamageAllocation {
                            attempt: domain.attempt(),
                            amount: amount.into(),
                        });
                    }
                    DecisionReply::AllocatePrevention(rows)
                }
                other => return Err(format!("unexpected damage decision {other:?}")),
            };
            self.act(actor, GameCommand::Reply { decision, choice })?;
        }
        Err("too many fixture damage decisions".into())
    }
    fn response(&mut self, case: &str) -> TraceResult<()> {
        let (actor, land, target) = match case {
            "D05" => (B, 500, "b1"),
            "D06" | "D07" => (B, 500, "a1"),
            "D08" => (A, 501, "a1"),
            _ => (A, 502, "a1"),
        };
        if self.holder()? != actor {
            self.pass()?;
        }
        self.fund(actor, &[land])?;
        self.act(
            actor,
            GameCommand::Cast {
                spell: self.object("response"),
                target: TargetRef::Object(self.object(target)),
            },
        )?;
        self.row(&format!("{case}-response-cast"))?;
        self.round()?;
        self.row(&format!("{case}-after-response"))?;
        Ok(())
    }
    fn output(self) -> Value {
        json!({"configuration":{"policy":"UnrestrictedFreeForAll","turn_order":self.state.execution().window().participants().iter().map(|&p|seat(p)).collect::<Vec<_>>(),"starting_life":self.starting_life},"rows":self.rows,"commands":self.commands,"choices":self.choices,"declaration_records":self.declarations,"assignment_records":self.assignments,"journals":self.journals,"check_journals":self.check_journals,"applied_damage_multiset":self.applied,"preventions":self.preventions,"payments":self.payments,"native_funding":self.funding,"native_only":self.native_only})
    }
}
fn run(case: &str) -> TraceResult<Value> {
    let mut f = Fixture::new(case)?;
    f.go(Step::BeginningCombat)?;
    f.row(&format!("{case}-before"))?;
    f.round()?;
    f.attack(if matches!(case, "D10" | "D11") {
        &[("a2", C), ("a1", B)]
    } else {
        &[("a1", B)]
    })?;
    f.row(&format!("{case}-attacker-priority"))?;
    if case == "D07" {
        f.response(case)?;
    }
    f.round()?;
    while let Some(p) = f.state.pending_decision() {
        let RuleFrame::Combat(CombatDecision::Blockers(_)) = p.frame() else {
            return Err("expected blocker decision".into());
        };
        if matches!(case, "D01" | "D07" | "D09" | "D10" | "D11") {
            f.block(&[])?;
        } else if matches!(case, "D03" | "D04" | "D13" | "D14" | "N01" | "N02") {
            f.block(&[("b1", "a1"), ("b2", "a1")])?;
        } else {
            f.block(&[("b1", "a1")])?;
        }
    }
    f.row(&format!("{case}-blocker-priority"))?;
    if matches!(
        case,
        "D05" | "D06" | "D08" | "D12" | "D13" | "D14" | "N01" | "N02"
    ) {
        f.response(case)?;
    }
    f.round()?;
    f.damage_choices(case)?;
    if case == "D09" {
        f.rows.push(json!({"id":"D09-outcome","comparison_scope":"common-outcome","life":{"A":f.state.execution().life_total(A).unwrap(),"B":f.state.execution().life_total(B).unwrap()},"game_over":f.state.outcome().is_some(),"winner":match f.state.outcome(){Some(GameOutcome::Winner(player))=>Some(seat(player)),_=>None},"losers":f.state.execution().state_based_losses().iter().map(|&p|seat(p)).collect::<Vec<_>>(),"outcome":format!("{:?}",f.state.outcome()),"priority":Value::Null,"check_journal":f.current_checks,"graveyards_top_to_bottom":f.graveyards()}));
        return Ok(f.output());
    }
    f.row(&format!("{case}-damage-priority"))?;
    f.round()?;
    f.row(&format!("{case}-endcombat"))?;
    f.round()?;
    f.row(&format!("{case}-postcombat-main"))?;
    Ok(f.output())
}
fn main() -> TraceResult<()> {
    let native_arguments = spellwake_reference_tools::creature_types::qualification_args()
        .unwrap_or_else(|error| {
            eprintln!("invalid qualification input: {error}");
            std::process::exit(1)
        });
    if !native_arguments.is_empty() {
        eprintln!("unsupported qualification arguments");
        std::process::exit(1);
    }

    spellwake_reference_tools::current_capture::start(
        "shared-damage",
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    )?;
    let mut scenarios = BTreeMap::new();
    let mut native_only = BTreeMap::new();
    for case in [
        "D01", "D02", "D03", "D04", "D05", "D06", "D07", "D08", "D09", "D10", "D11", "D12", "D13",
        "D14",
    ] {
        scenarios.insert(case, run(case)?);
    }
    for case in ["N01", "N02"] {
        native_only.insert(case, run(case)?);
    }
    let mut observations = serde_json::json!({"schema_version":3,"profile":"shared-damage","scenarios":scenarios,"native_only_profiles":native_only,"limits":["No reference or expected input is read","Native mana is funded by real authored land actions; Forge independently seeds floating mana","N01/N02 are actual root combat, N03/N04 are separately executed private primitive tests","D13/D14/N01/N02 explicitly choose B entrants b1 then b2 in final top-to-bottom order","GameBasis context uses the shared six-field typed current projection","No complete card/game/format/capacity qualification"]});
    spellwake_reference_tools::capture_points::bind_value_points(
        &mut observations,
        "/observations",
    )?;
    let envelope =
        spellwake_reference_tools::current_capture::finish("shared-damage", observations)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).expect("serialize actual current capture")
    );
    Ok(())
}
