//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! Actual native declarations and responses; no expected/reference files are read.
use serde_json::{Value, json};
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::{
    combat::{
        AttackDeclaration, BlockDeclaration, BlockStatus, CombatDecision, CombatDeclarationRecord,
        CombatRules, DeclarationOrigin,
    },
    effects::{DecisionReply, Instruction, Program, SeedCard},
    game::{GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{Step, TurnStage, TurnStart},
};
use spellwake_reference_tools::damage_trace::damage_journal_view;
use std::collections::BTreeMap;
const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);
const C: SeatId = SeatId(99);
const D: SeatId = SeatId(7);
type TraceResult<T> = Result<T, String>;
fn seat(p: SeatId) -> &'static str {
    match p {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        _ => panic!("unknown fixture seat"),
    }
}
fn phase(s: Step) -> &'static str {
    match s {
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
fn creature(id: u64, owner: SeatId, hand: bool) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: if hand {
            ObjectDestination::Hand
        } else {
            ObjectDestination::Battlefield { controller: owner }
        },
        program: Program::creature(
            Some(ManaCost {
                generic: 4.into(),
                required: BTreeMap::from([(ManaType::Green, 2.into())]),
            }),
            6.into(),
            4.into(),
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
    rows: Vec<Value>,
    commands: Vec<Value>,
    choices: Vec<Value>,
    declarations: Vec<Value>,
    payments: Vec<Value>,
    funding: Vec<Value>,
    native_only: Vec<Value>,
}
impl Fixture {
    fn new(case: &str) -> TraceResult<Self> {
        let order = if case == "C08" {
            vec![A, D, C, B]
        } else {
            vec![A, B]
        };
        let mut primary = BTreeMap::new();
        let mut cards = vec![];
        let mut add = |label: &str, card: SeedCard| {
            primary.insert(label.to_owned(), card.card);
            cards.push(card);
        };
        if case == "C01" {
            add(
                "rod",
                SeedCard {
                    card: CardId(91),
                    owner: A,
                    destination: ObjectDestination::Battlefield { controller: A },
                    program: Program::artifact(None, vec![]),
                },
            );
        } else {
            add("a1", creature(1, A, false));
        }
        if ["C03", "C04", "C05", "C06", "C07", "C08"].contains(&case) {
            add("b1", creature(10, B, false));
        }
        if case == "C03" {
            add("freshA", creature(90, A, true));
            add(
                "rod",
                SeedCard {
                    card: CardId(91),
                    owner: A,
                    destination: ObjectDestination::Battlefield { controller: A },
                    program: Program::artifact(None, vec![]),
                },
            );
        }
        if case == "C04" {
            add("b2", creature(11, B, false));
        }
        if case == "C08" {
            add("a2", creature(2, A, false));
            add("c1", creature(20, C, false));
            add("d1", creature(30, D, false));
        }
        if case == "C09" {
            add(
                "rod",
                SeedCard {
                    card: CardId(91),
                    owner: B,
                    destination: ObjectDestination::Battlefield { controller: B },
                    program: Program::artifact(None, vec![]),
                },
            );
        }
        if ["C05", "C06", "C07"].contains(&case) {
            add(
                "bounce",
                SeedCard {
                    card: CardId(92),
                    owner: B,
                    destination: ObjectDestination::Hand,
                    program: Program::instant(
                        ManaCost::one(ManaType::Blue),
                        ExecutableBody::new(
                            BodyTarget::SingleCreature,
                            vec![Instruction::ReturnToOwnersHand],
                        ),
                    ),
                },
            );
        }
        for (index, &owner) in order.iter().enumerate() {
            for n in 0..8 {
                cards.push(land(
                    1000 + index as u64 * 100 + n,
                    owner,
                    ManaType::Blue,
                    ObjectDestination::LibraryBottom,
                ));
            }
        }
        // Disclosed native-only support: actual authored land abilities, not a
        // trusted mid-game resource credit or a claim about a printed land.
        cards.push(land(
            500,
            B,
            ManaType::Blue,
            ObjectDestination::Battlefield { controller: B },
        ));
        for n in 0..6 {
            cards.push(land(
                510 + n,
                A,
                if n < 2 {
                    ManaType::Green
                } else {
                    ManaType::Colourless
                },
                ObjectDestination::Battlefield { controller: A },
            ));
        }
        let context = GameContext::new(
            "combat-declarations".into(),
            spellwake_reference_tools::creature_types::selected_vocabulary()?
                .rules_revision()
                .to_owned(),
            "authored".into(),
            "literal".into(),
            "explicit-unrestricted-FFA".into(),
            spellwake_reference_tools::creature_types::selected_vocabulary()?.id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .map_err(|e| format!("{e:?}"))?;
        let state = spellwake_reference_tools::current_capture::initialize_scheduled(
            GameId(702),
            context,
            {
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
            },
            TurnStart::default(),
        )
        .map_err(|e| format!("{e:?}"))?
        .into_state();
        Ok(Self {
            state,
            primary,
            rows: vec![],
            commands: vec![],
            choices: vec![],
            declarations: vec![],
            payments: vec![],
            funding: vec![],
            native_only: vec![],
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
    fn assert_frame_scope(&self) {
        assert!(
            !self
                .state
                .pending_decision()
                .is_some_and(|pending| matches!(pending.frame(), RuleFrame::Triggers(_))),
            "bounded combat-declaration projection excludes trigger-order frames"
        );
    }
    fn protocol(&self) -> Value {
        self.assert_frame_scope();
        let execution = self.state.execution();
        let priority = match execution.window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Some(seat(holder)),
            _ => None,
        };
        json!({"basis_revision": self.state.basis().revision,
            "executor_revision": execution.revision(),
            "active": seat(execution.window().turn_owner()), "priority": priority,
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
                CombatDeclarationRecord::Attackers{player,origin,attackers,taps}=>self.declarations.push(json!({"kind":"attackers","actor":seat(*player),"origin":match origin{DeclarationOrigin::PlayerReply=>"PlayerReply",DeclarationOrigin::ForcedEmpty=>"ForcedEmpty"},"selected":attackers.iter().map(|d|json!({"attacker":self.alias(d.attacker),"defender":seat(d.defender)})).collect::<Vec<_>>(),"taps":taps.iter().map(|t|json!({"card":self.label(t.before.card_id().expect("source-authored card-only trace")),"before":t.before.tapped(),"after":t.after.tapped(),"same_incarnation":t.before.id()==t.after.id(),"same_control_epoch":t.before.control_epoch()==t.after.control_epoch()})).collect::<Vec<_>>()})),
                CombatDeclarationRecord::Blockers{player,origin,blockers}=>self.declarations.push(json!({"kind":"blockers","actor":seat(*player),"origin":match origin{DeclarationOrigin::PlayerReply=>"PlayerReply",DeclarationOrigin::ForcedEmpty=>"ForcedEmpty"},"selected":blockers.iter().map(|d|json!({"blocker":self.alias(d.blocker),"attacker":self.alias(d.attacker)})).collect::<Vec<_>>()})),
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
            self.funding.push(json!({"actor":seat(production.controller),"source":format!("{:?}",production.source),"produced":production.produced.iter().map(|(t,n)|(format!("{t:?}"),n.clone())).collect::<BTreeMap<_,_>>() }));
        }
        let damage_journal = damage_journal_view(candidate.damage_journal());
        self.state = candidate.into_state();
        self.commands.push(spellwake_reference_tools::capture_points::mark_value("Fixture::commands actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),&spellwake_reference_tools::packet::AliasContext::default(),json!({"actor":seat(actor),"command":command_text,"intent":intent,"objects_before":objects,"before":before,"after":self.protocol(),"basis_revision":before_revision,"next_revision":self.state.basis().revision,"step_after":phase(self.step()),"damage_journal":damage_journal})).expect("collect actual scoped native point"));
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
    fn snapshot(&self, id: &str, scope: &str) -> Value {
        self.assert_frame_scope();
        let combat = self.state.execution().combat();
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
            cards.insert(alias.clone(),json!({"owner":seat(current.owner()),"controller":current.controller().map(seat),"zone":zone,"tapped":current.tapped(),"continuous":current.has_continuous_control_since_turn_start(),"attacking":a.is_some(),"blocking":b.is_some(),"defender":a.map(|a|seat(a.defender())),"block_status":a.map(|a|match a.block_status(){BlockStatus::NotDeclared=>"NotDeclared",BlockStatus::Unblocked=>"Unblocked",BlockStatus::Blocked=>"Blocked"}),"attackers_blocked":linked,"blockers":blockers}));
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
        json!({"id":id,"comparison_scope":scope,"step":phase(self.step()),"active":seat(self.state.execution().window().turn_owner()),"priority":priority,"stack_size":self.state.execution().window().entries().len(),"cards":cards,"combat":{"attackers":attackers,"blockers":blockers},"native":{"basis_revision":self.state.basis().revision,"executor_revision":self.state.execution().revision(),"combat_context_present":combat.is_some(),"had_attackers":combat.map(|c|c.had_attackers()),"stage":format!("{:?}",self.state.turn().unwrap().stage()),"pending_chooser":self.state.pending_decision().map(|p|seat(p.chooser()))}})
    }
    fn row(&mut self, id: &str) {
        self.rows.push(
            spellwake_reference_tools::capture_points::mark_value(
                "Fixture::rows actual writer",
                spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),
                &spellwake_reference_tools::packet::AliasContext::default(),
                self.snapshot(id, "common"),
            )
            .expect("collect actual scoped native point"),
        );
    }
    fn tail(&mut self, id: &str) {
        self.native_only.push(
            spellwake_reference_tools::capture_points::mark_value(
                "Fixture::native_only actual writer",
                spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),
                &spellwake_reference_tools::packet::AliasContext::default(),
                self.snapshot(id, "native-only-tail"),
            )
            .expect("collect actual scoped native point"),
        );
    }
    fn output(self) -> Value {
        let execution = self.state.execution();
        let configuration = json!({
            "policy": format!("{:?}", execution.combat_rules()),
            "turn_order": execution.window().participants().iter().map(|&p| seat(p)).collect::<Vec<_>>(),
            "life": execution.window().participants().iter().map(|&p| (seat(p), execution.life_total(p).unwrap().clone())).collect::<BTreeMap<_, _>>(),
        });
        json!({"configuration":configuration,"rows":self.rows,"commands":self.commands,"choices":self.choices,"declaration_records":self.declarations,"payments":self.payments,"native_funding":self.funding,"native_only_tail":self.native_only})
    }
}
fn run(case: &str) -> TraceResult<Value> {
    let mut f = Fixture::new(case)?;
    if case == "C01" || case == "C02" {
        f.go(Step::BeginningCombat)?;
        f.row(&format!("{case}-before"));
        f.round()?;
        if case == "C02" {
            f.attack(&[])?;
        }
        f.row(&format!("{case}-attacker-priority"));
        f.round()?;
        f.row(&format!("{case}-endcombat-priority"));
        f.round()?;
        f.row(&format!("{case}-postcombat-main"));
        return Ok(f.output());
    }
    if case == "C03" {
        f.go(Step::PrecombatMain)?;
        f.fund(A, &[510, 511, 512, 513, 514, 515])?;
        let spell = f.object("freshA");
        f.act(
            A,
            GameCommand::CastUntargeted {
                spell,
                payment: Some(ManaPayment {
                    spend: vec![
                        ManaSpend {
                            mana_type: ManaType::Green,
                            amount: 2.into(),
                        },
                        ManaSpend {
                            mana_type: ManaType::Colourless,
                            amount: 4.into(),
                        },
                    ],
                }),
            },
        )?;
        f.round()?;
        f.row("C03-cast-resolved");
    }
    f.go(Step::DeclareAttackers)?;
    if case == "C03" || case == "C04" {
        f.row(&format!("{case}-before-attacks"));
    }
    f.attack(if case == "C08" {
        &[("a2", C), ("a1", B)]
    } else {
        &[("a1", B)]
    })?;
    if case != "C05" && case != "C06" {
        f.row(&format!("{case}-attacker-priority"));
    }
    if case == "C07" {
        f.pass()?;
        f.fund(B, &[500])?;
        let spell = f.object("bounce");
        let target = TargetRef::Object(f.object("a1"));
        f.act(B, GameCommand::Cast { spell, target })?;
        f.row("C07-response-cast");
        f.round()?;
        f.row("C07-after-bounce");
        f.round()?;
        f.tail("C07-normative-blocker-priority");
        f.round()?;
        // Schema 1's retained native packet stopped at missing damage work. The
        // current engine executes this empty work and grants combat-damage priority.
        if f.step() != Step::CombatDamage
            || f.state.turn().unwrap().stage() != TurnStage::Priority
            || f.state.pending_decision().is_some()
            || f.state.execution().pending_damage().is_some()
        {
            return Err("expected actual empty combat-damage completion and priority".into());
        }
        f.holder()?;
        f.tail("C07-normative-combat-damage-priority");
        return Ok(f.output());
    }
    f.round()?;
    if case == "C04" {
        f.row("C04-before-blocks");
    }
    if case == "C08" {
        f.tail("C08-before-blocks-C");
        f.block(&[("c1", "a2")])?;
        f.tail("C08-before-blocks-B");
        f.block(&[("b1", "a1")])?;
    } else if case == "C03" {
        f.block(&[])?;
    } else if case != "C09" {
        if case == "C04" {
            f.block(&[("b1", "a1"), ("b2", "a1")])?;
        } else {
            f.block(&[("b1", "a1")])?;
        }
    }
    if case == "C05" || case == "C06" {
        f.row(&format!("{case}-blocks-settled"));
        f.pass()?;
        f.fund(B, &[500])?;
        let spell = f.object("bounce");
        let target = TargetRef::Object(f.object(if case == "C05" { "b1" } else { "a1" }));
        f.act(B, GameCommand::Cast { spell, target })?;
        f.row(&format!("{case}-response-cast"));
        f.round()?;
        f.row(&format!("{case}-after-bounce"));
    } else {
        f.row(&format!("{case}-blocker-priority"));
    }
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
        "declarations",
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    )?;
    let mut cases = BTreeMap::new();
    for case in [
        "C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09",
    ] {
        cases.insert(case, run(case)?);
    }
    let mut observations = serde_json::json!({"schema_version":2,"profile":"combat-declarations","native_contract":"shared-damage-v2","historical_native_contract":"combat-declarations/schema1 retained separately","scenarios":cases,"limits":["All native observations come from actual root inputs; C07 now observes actual empty combat-damage work completion and priority, with no completion bypass or mid-game resource reseed","Native-only authored mana support uses real existing land abilities; support-card state is not joined to Forge floating setup","C07 normative tail is not forced to match Forge's verified historical-skip discrepancy","No full card/game/format/variant/replay or capacity qualification"]});
    spellwake_reference_tools::capture_points::bind_value_points(
        &mut observations,
        "/observations",
    )?;
    let envelope =
        spellwake_reference_tools::current_capture::finish("declarations", observations)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).expect("serialize actual current capture")
    );
    Ok(())
}
