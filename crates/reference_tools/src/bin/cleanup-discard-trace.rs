//! This finite producer authors only physical-card seeds. Physical provenance
//! assertions below check that fixture invariant, not a generic object fallback.
//! Actual native cleanup/check journeys and one explicitly separate movement
//! primitive. This executable reads no expected states or reference artifacts.
use serde_json::{Value, json};
use spellwake_engine::program::{BodyTarget, ExecutableBody};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    checks::{CheckBoundary, CheckDecision},
    combat::{
        AttackDeclaration, BlockDeclaration, CombatDamageAssignment, CombatDecision,
        CombatDeclarationRecord, CombatRules,
    },
    damage::DamageRecipient,
    draw::DrawEvent,
    effects::{DecisionReply, Instruction, Program, SeedCard},
    game::{
        GameBasis, GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate,
        RuleFrame,
    },
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, GameObjects, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    setup::GameSeed,
    stack_window::{StackKind, WindowPhase},
    targets::TargetRef,
    turn::{Step, TurnStart},
};
use spellwake_reference_tools::{
    check_trace::{check_decision_view, check_game_object_view, check_journal_view},
    damage_trace::{damage_journal_view, turn_position_view},
};
use std::collections::{BTreeMap, BTreeSet};

const A: SeatId = SeatId(41);
const B: SeatId = SeatId(3);
const C: SeatId = SeatId(99);
type TraceResult<T> = Result<T, String>;

fn seat(player: SeatId) -> &'static str {
    match player {
        A => "A",
        B => "B",
        C => "C",
        _ => panic!("unknown authored seat"),
    }
}
fn basis(b: &GameBasis) -> Value {
    json!({"game":b.game.0,"context":spellwake_reference_tools::projection::context(&b.context),"epoch":b.epoch,"revision":b.revision})
}
fn zone(z: ObjectZone) -> &'static str {
    match z {
        ObjectZone::Library(_) => "Library",
        ObjectZone::Hand(_) => "Hand",
        ObjectZone::Graveyard(_) => "Graveyard",
        ObjectZone::Battlefield => "Battlefield",
        ObjectZone::Stack => "Stack",
        ObjectZone::Exile => "Exile",
        ObjectZone::Command => "Command",
    }
}
fn color(name: &str) -> ManaType {
    match name {
        "Island" => ManaType::Blue,
        "Plains" => ManaType::White,
        "Mountain" => ManaType::Red,
        _ => panic!("authored basic land"),
    }
}
fn land_program(name: &str) -> Program {
    Program::land(
        std::collections::BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(1),
            output: ManaOutput::Fixed(BTreeMap::from([(color(name), 1.into())])),
        }],
    )
}
fn creature_program(wurm: bool, doomed: bool) -> Program {
    Program::creature(
        None,
        if wurm { 6.into() } else { 3.into() },
        if doomed {
            0.into()
        } else if wurm {
            4.into()
        } else {
            3.into()
        },
        std::collections::BTreeSet::new(),
    )
}
fn pyrite_program() -> Program {
    Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::one(ManaType::Red),
            source_cost: SourceCost::Sacrifice,
            body: ExecutableBody::new(
                BodyTarget::SINGLE_CREATURE,
                vec![Instruction::DealDamage(Box::new(
                    spellwake_engine::toolbox::DealDamageSpec {
                        source: spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                        recipients: spellwake_engine::toolbox::DamageTargetQuery::Targets,
                        amount: spellwake_engine::program::DamageAmountSpec::Expr(
                            spellwake_engine::program::ValueExpr::literal(2),
                        ),
                        exile_if_dies: false,
                    },
                ))],
            ),
        }],
    )
}
fn bounce_program() -> Program {
    Program::instant(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(
            BodyTarget::SINGLE_CREATURE,
            vec![Instruction::ZoneOp(Box::new(
                spellwake_engine::toolbox::ZoneOpSpec::move_all(
                    spellwake_engine::toolbox::ObjectQuery::Targets,
                    spellwake_engine::toolbox::ZoneDest::Hand,
                ),
            ))],
        ),
    )
}

#[derive(Clone)]
struct Identity {
    alias: String,
    name: String,
    common: bool,
}
#[derive(Default)]
struct SeedBuilder {
    cards: Vec<SeedCard>,
    identities: BTreeMap<CardId, Identity>,
}
impl SeedBuilder {
    fn add(
        &mut self,
        alias: &str,
        name: &str,
        owner: SeatId,
        destination: ObjectDestination,
        program: Program,
        common: bool,
    ) {
        let card = CardId(self.cards.len() as u64 + 1);
        self.identities.insert(
            card,
            Identity {
                alias: alias.into(),
                name: name.into(),
                common,
            },
        );
        self.cards.push(SeedCard {
            card,
            owner,
            destination,
            program,
        });
    }
    fn land(
        &mut self,
        alias: &str,
        name: &str,
        owner: SeatId,
        destination: ObjectDestination,
        common: bool,
    ) {
        self.add(alias, name, owner, destination, land_program(name), common);
    }
    fn creature(
        &mut self,
        alias: &str,
        owner: SeatId,
        controller: SeatId,
        wurm: bool,
        doomed: bool,
    ) {
        self.add(
            alias,
            if wurm { "Craw Wurm" } else { "Hill Giant" },
            owner,
            ObjectDestination::Battlefield { controller },
            creature_program(wurm, doomed),
            true,
        );
    }
}

struct Fixture {
    state: GameState,
    identities: BTreeMap<CardId, Identity>,
    rows: Vec<Value>,
    native_only_rows: Vec<Value>,
    commands: Vec<Value>,
    initialization: Value,
    seed: Value,
    current_facts: Value,
}
impl Fixture {
    fn new(case: &str) -> TraceResult<Self> {
        let participants = if case == "C08" {
            vec![A, C, B]
        } else {
            vec![A, B]
        };
        let active = if case == "C08" { C } else { A };
        let start = match case {
            "C01" | "C02" | "C03" | "C04" => Step::End,
            "C05" => Step::Cleanup,
            "C06" | "C07" | "C08" => Step::Untap,
            "C09" | "C10" | "C11" => Step::PrecombatMain,
            _ => return Err(format!("unknown root case {case}")),
        };
        let mut builder = SeedBuilder::default();
        for &owner in &participants {
            builder.land(
                &format!("{}-old-bottom", seat(owner)),
                "Island",
                owner,
                ObjectDestination::Graveyard,
                true,
            );
            builder.land(
                &format!("{}-old-top", seat(owner)),
                "Plains",
                owner,
                ObjectDestination::Graveyard,
                true,
            );
        }
        let hand_count = match case {
            "C01" => 10,
            "C02" => 7,
            "C04" | "C05" => 9,
            _ => 0,
        };
        for index in 0..hand_count {
            builder.land(
                &format!("h{index}"),
                if index % 2 == 0 { "Island" } else { "Plains" },
                A,
                ObjectDestination::Hand,
                true,
            );
        }
        match case {
            "C04" => {
                builder.creature("giant", A, A, false, false);
                builder.add(
                    "pyrite",
                    "Pyrite Spellbomb",
                    B,
                    ObjectDestination::Battlefield { controller: B },
                    pyrite_program(),
                    true,
                );
            }
            "C05" => {
                builder.creature("doomed", A, A, false, true);
                builder.creature("returnee", A, A, false, false);
                builder.add(
                    "response",
                    "Unsummon",
                    B,
                    ObjectDestination::Hand,
                    bounce_program(),
                    true,
                );
            }
            "C06" | "C07" => {
                builder.creature("a1", A, A, true, false);
                builder.creature("b1", B, B, false, false);
                builder.creature("b2", B, B, false, false);
            }
            "C08" => {
                for (alias, owner, controller) in [
                    ("a1", A, C),
                    ("a2", A, C),
                    ("c1", C, C),
                    ("b1", B, B),
                    ("b2", B, B),
                    ("c2", C, A),
                ] {
                    builder.creature(alias, owner, controller, true, false);
                }
            }
            "C09" => {
                builder.add(
                    "response",
                    "Unsummon",
                    A,
                    ObjectDestination::Hand,
                    bounce_program(),
                    true,
                );
                builder.add(
                    "counter",
                    "Counterspell",
                    A,
                    ObjectDestination::Hand,
                    Program::instant(
                        ManaCost::specific(ManaType::Blue, 2.into()),
                        ExecutableBody::new(
                            BodyTarget::single_spell(spellwake_engine::program::SpellFilter::any()),
                            vec![Instruction::StackOp(Box::new(
                                spellwake_engine::toolbox::StackOpSpec::counter_targeted(
                                    spellwake_engine::toolbox::CounterDestination::Graveyard,
                                ),
                            ))],
                        ),
                    ),
                    true,
                );
                builder.creature("giant", B, B, false, false);
            }
            "C10" => {
                builder.add(
                    "source",
                    "Arc Lightning",
                    A,
                    ObjectDestination::Hand,
                    Program::sorcery(
                        ManaCost {
                            generic: 2.into(),
                            required: BTreeMap::from([(ManaType::Red, 1.into())]),
                        },
                        ExecutableBody::new(
                            BodyTarget::SINGLE_CREATURE,
                            vec![Instruction::DealDamage(Box::new(
                                spellwake_engine::toolbox::DealDamageSpec {
                                    source:
                                        spellwake_engine::toolbox::DamageSourceQuery::EntrySource,
                                    recipients:
                                        spellwake_engine::toolbox::DamageTargetQuery::Targets,
                                    amount: spellwake_engine::program::DamageAmountSpec::Expr(
                                        spellwake_engine::program::ValueExpr::literal(3),
                                    ),
                                    exile_if_dies: false,
                                },
                            ))],
                        ),
                    ),
                    true,
                );
                builder.creature("giant", A, A, false, false);
            }
            "C11" => {
                for alias in ["pyrite1", "pyrite2"] {
                    builder.add(
                        alias,
                        "Pyrite Spellbomb",
                        A,
                        ObjectDestination::Battlefield { controller: A },
                        pyrite_program(),
                        true,
                    );
                }
                builder.creature("giant", A, A, false, false);
            }
            _ => {}
        }
        let funding: Vec<(SeatId, &str)> = match case {
            "C04" => vec![(B, "Mountain")],
            "C05" => vec![(B, "Island")],
            "C09" => vec![(A, "Island"); 3],
            "C10" => vec![(A, "Mountain"); 3],
            "C11" => vec![(A, "Mountain"); 2],
            _ => vec![],
        };
        for (index, (owner, name)) in funding.into_iter().enumerate() {
            builder.land(
                &format!("fund-{}-{index}", seat(owner)),
                name,
                owner,
                ObjectDestination::Battlefield { controller: owner },
                false,
            );
        }
        for &owner in &participants {
            for index in 0..64 {
                builder.land(
                    &format!("{}-library-{index}", seat(owner)),
                    "Island",
                    owner,
                    ObjectDestination::LibraryBottom,
                    false,
                );
            }
        }
        let seed_view = json!({"mode":"explicit-native-GameSeed","creature_vocabulary":spellwake_reference_tools::creature_types::selected_vocabulary()?.id().to_string(),"active":seat(active),"participants":participants.iter().map(|p|seat(*p)).collect::<Vec<_>>(),"start":{"number":1,"step":format!("{start:?}")},"mana":[],"life":participants.iter().map(|p|(seat(*p),"20")).collect::<BTreeMap<_,_>>(),"cards_in_seed_chronology":builder.cards.iter().map(|c|json!({"physical_card":c.card.0,"alias":builder.identities[&c.card].alias,"name":builder.identities[&c.card].name,"common":builder.identities[&c.card].common,"owner":seat(c.owner),"destination":format!("{:?}",c.destination),"authored_program":spellwake_reference_tools::program::view(&c.program)})).collect::<Vec<_>>(),"prepared_override":if case=="C05" {json!({"alias":"doomed","printed":"3/3","authored":"3/0","duration":"persistent authored fixture, no expiry claim"})}else{Value::Null},"prepared_foreign_control":case=="C08","control_history":"only real turn-start hook at Untap; no manual stamp","common_hand_projection":"only tracked case cards; all actual auxiliary hand cards remain in all_hands and all_cards","library_count_per_seat":64,"funding":"actual supported land taps; no seeded or direct floating credit"});
        let seed = {
            let cards: Vec<SeedCard> = builder.cards;
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
                participants,
                active,
                cards,
                initial_battlefield_order,
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                spellwake_reference_tools::creature_types::selected_vocabulary()?,
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        };
        let candidate = spellwake_reference_tools::current_capture::initialize_scheduled(
            GameId(
                1900 + u128::from(case.as_bytes()[2] - b'0')
                    + if case == "C10" || case == "C11" {
                        10
                    } else {
                        0
                    },
            ),
            GameContext::new(
                "cleanup-discard".into(),
                spellwake_reference_tools::creature_types::selected_vocabulary()?
                    .rules_revision()
                    .to_owned(),
                "authored-bounded-printing-facts".into(),
                "native-source".into(),
                "unrestricted-FFA".into(),
                spellwake_reference_tools::creature_types::selected_vocabulary()?.id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .map_err(|e| format!("{e:?}"))?,
            seed,
            TurnStart {
                number: 1,
                step: start,
            },
        )
        .map_err(|e| format!("initialize {case}: {e:?}"))?;
        let mut fixture = Self {
            state: candidate.state().clone(),
            identities: builder.identities,
            rows: vec![],
            native_only_rows: vec![],
            commands: vec![],
            initialization: Value::Null,
            seed: seed_view,
            current_facts: Value::Null,
        };
        fixture.current_facts =
            fixture.facts(&candidate, candidate.state().execution().window().cards())?;
        fixture.initialization = spellwake_reference_tools::capture_points::mark_value(
            "Fixture::new actual initialization",
            spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&fixture.state),
            &spellwake_reference_tools::packet::AliasContext::default(),
            json!({"base":spellwake_reference_tools::projection::candidate_base(candidate.base()),"facts":fixture.current_facts,"state":fixture.snapshot()?}),
        )?;
        Ok(fixture)
    }
    fn label(&self, card: CardId) -> String {
        self.identities[&card].alias.clone()
    }
    fn card(&self, alias: &str) -> TraceResult<CardId> {
        self.identities
            .iter()
            .find(|(_, identity)| identity.alias == alias)
            .map(|(id, _)| *id)
            .ok_or_else(|| format!("unknown authored alias {alias}"))
    }
    fn object(&self, alias: &str) -> TraceResult<ObjectId> {
        self.state
            .execution()
            .window()
            .cards()
            .current_identity(self.card(alias)?)
            .ok_or_else(|| format!("no current incarnation for {alias}"))
    }
    fn alias(&self, id: ObjectId) -> TraceResult<String> {
        self.state
            .execution()
            .window()
            .cards()
            .get(id)
            .map(|c| self.label(c.card_id().expect("source-authored card-only trace")))
            .ok_or_else(|| format!("stale exact object {id:?}"))
    }
    fn aliases(&self, ids: &[ObjectId]) -> TraceResult<Vec<String>> {
        ids.iter().map(|id| self.alias(*id)).collect()
    }
    fn require_domain(&self, ids: &[ObjectId], expected: &[&str]) -> TraceResult<()> {
        let actual = self.aliases(ids)?;
        if actual.iter().collect::<BTreeSet<_>>().len() != actual.len()
            || actual.iter().map(String::as_str).collect::<BTreeSet<_>>()
                != expected.iter().copied().collect()
        {
            return Err(format!(
                "domain differs: actual {actual:?}, authored {expected:?}"
            ));
        }
        Ok(())
    }
    fn holder(&self) -> TraceResult<SeatId> {
        match self.state.execution().window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => Ok(holder),
            other => Err(format!("expected actual priority, got {other:?}")),
        }
    }
    fn step(&self) -> Step {
        self.state
            .turn()
            .expect("scheduled native case")
            .position()
            .step
    }
    fn recipient(&self, target: DamageRecipient) -> TraceResult<Value> {
        match target {
            DamageRecipient::Creature(id)
            | DamageRecipient::Planeswalker(id)
            | DamageRecipient::Battle(id) => {
                Ok(json!({"kind":"creature","alias":self.alias(id)?,"object":format!("{id:?}")}))
            }
            DamageRecipient::Player(player) => Ok(json!({"kind":"player","player":seat(player)})),
        }
    }
    fn target(&self, target: TargetRef) -> TraceResult<Value> {
        match target {
            TargetRef::Object(id) => {
                Ok(json!({"kind":"card","alias":self.alias(id)?,"object":format!("{id:?}")}))
            }
            TargetRef::Player(p) => Ok(json!({"kind":"player","player":seat(p)})),
            TargetRef::Stack(id) => Ok(json!({"kind":"stack","entry":format!("{id:?}")})),
        }
    }
    fn decision(&self) -> TraceResult<Value> {
        let Some(p) = self.state.pending_decision() else {
            return Ok(Value::Null);
        };
        let frame = match p.frame() {
            RuleFrame::Checks(f) => json!(check_decision_view(
                f,
                self.state.execution().window().cards(),
                |id| self.label(id)
            )?),
            RuleFrame::Combat(CombatDecision::Attackers(f)) => {
                json!({"kind":"declare_attackers","revision":f.revision(),"chooser":f.chooser().0,"eligible":self.aliases(f.eligible_attackers())?,"eligible_exact":f.eligible_attackers().iter().map(|id|format!("{id:?}")).collect::<Vec<_>>(),"defenders":f.defenders().iter().map(|p|seat(*p)).collect::<Vec<_>>()})
            }
            RuleFrame::Combat(CombatDecision::Blockers(f)) => {
                json!({"kind":"declare_blockers","revision":f.revision(),"chooser":f.chooser().0,"eligible":self.aliases(f.eligible_blockers())?,"eligible_exact":f.eligible_blockers().iter().map(|id|format!("{id:?}")).collect::<Vec<_>>(),"attackers":self.aliases(f.attackers())?})
            }
            RuleFrame::Combat(CombatDecision::DamageAssignments(f)) => {
                json!({"kind":"assign_combat_damage","revision":f.revision(),"chooser":f.chooser().0,"position":turn_position_view(f.position()),"domains":f.domains().iter().map(|d|Ok(json!({"source":self.alias(d.source())?,"source_object":format!("{:?}",d.source()),"power":d.power(),"recipients":d.recipients().iter().map(|r|self.recipient(*r)).collect::<TraceResult<Vec<_>>>()?}))).collect::<TraceResult<Vec<Value>>>()?})
            }
            other => return Err(format!("unexpected native prompt {other:?}")),
        };
        Ok(
            json!({"id":format!("{:?}",p.id()),"basis":basis(p.basis()),"chooser":seat(p.chooser()),"frame":frame}),
        )
    }
    fn snapshot(&self) -> TraceResult<Value> {
        let execution = self.state.execution();
        let window = execution.window();
        let registry = window.cards();
        let evaluated = execution
            .evaluated_characteristics()
            .map_err(|error| format!("characteristic evaluation failed: {error:?}"))?;
        let mut common = serde_json::Map::new();
        let mut all = Vec::new();
        for object in registry.iter() {
            let identity =
                &self.identities[&object.card_id().expect("source-authored card-only trace")];
            let creature = evaluated
                .get(object.id())
                .and_then(|value| value.power_toughness.as_ref());
            let counters = execution.power_toughness_counters(object.id());
            let view = json!({"name":identity.name,"owner":seat(object.owner()),"controller":object.controller().map(seat),"zone":zone(object.zone()),"tapped":object.tapped().unwrap_or(false),"physical_tapped":object.tapped(),"power":creature.as_ref().map(|c|c.derived.power.clone()),"toughness":creature.as_ref().map(|c|c.derived.toughness.clone()),"marked_damage":execution.battlefield_marked_damage(object.id()),"counters":counters.map(|c|json!({"plus_one":c.plus_one,"minus_one":c.minus_one})),"physical":check_game_object_view(object,|id|self.label(id))});
            if identity.common {
                common.insert(identity.alias.clone(), view.clone());
            }
            all.push(json!({"alias":identity.alias,"common":identity.common,"state":view}));
        }
        let mut hands = BTreeMap::new();
        let mut all_hands = BTreeMap::new();
        let mut piles = BTreeMap::new();
        let mut physical_piles = BTreeMap::new();
        let mut libraries = BTreeMap::new();
        let mut life = BTreeMap::new();
        for &player in window.participants() {
            let hand: Vec<_> = registry
                .iter()
                .filter(|c| c.zone() == ObjectZone::Hand(player))
                .collect();
            hands.insert(
                seat(player),
                hand.iter()
                    .filter(|c| {
                        self.identities[&c.card_id().expect("source-authored card-only trace")]
                            .common
                    })
                    .map(|c| self.label(c.card_id().expect("source-authored card-only trace")))
                    .collect::<Vec<_>>(),
            );
            all_hands.insert(
                seat(player),
                hand.iter()
                    .map(|c| self.label(c.card_id().expect("source-authored card-only trace")))
                    .collect::<Vec<_>>(),
            );
            piles.insert(seat(player), self.aliases(registry.graveyard(player))?);
            physical_piles.insert(
                seat(player),
                registry
                    .graveyard(player)
                    .iter()
                    .map(|id| {
                        check_game_object_view(registry.get(*id).expect("canonical pile"), |id| {
                            self.label(id)
                        })
                    })
                    .collect::<Vec<_>>(),
            );
            libraries.insert(seat(player), self.aliases(registry.library(player))?);
            life.insert(
                seat(player),
                execution
                    .life_total(player)
                    .ok_or("missing explicit life")?
                    .clone(),
            );
        }
        let stack=window.entries().iter().map(|entry| {
            let kind=match entry.kind() {
                StackKind::CardSpell(id)=>json!({"kind":"CardSpell","origin":self.alias(*id)?,"object":format!("{id:?}")}),
                StackKind::ActivatedAbility{source}=>json!({"kind":"Ability","origin":self.label(source.card_id().expect("source-authored card-only trace")),"source":check_game_object_view(source,|id|self.label(id))}),
                StackKind::TriggeredAbility{source}=>json!({"kind":"TriggeredAbility","origin":self.label(source.card_id().expect("source-authored card-only trace")),"source":check_game_object_view(source,|id|self.label(id))}),
                StackKind::Speed=>json!({"kind":"Speed"}),
                StackKind::Monarch=>json!({"kind":"Monarch"}),
                StackKind::DelayedCopy{original}=>json!({"kind":"DelayedCopy","original":format!("{original:?}")}),
                StackKind::Emblem{emblem}=>json!({"kind":"Emblem","id":emblem.sequence()}),
                StackKind::SpellCopy{copied_from,owner}=>json!({"kind":"SpellCopy","copied_from":format!("{copied_from:?}"),"owner":seat(*owner)}),
            };
            Ok(json!({"entry":format!("{:?}",entry.id()),"controller":seat(entry.controller()),"kind":kind,"targets":format!("{:?}",entry.targets())}))
        }).collect::<TraceResult<Vec<Value>>>()?;
        Ok(
            json!({"state":{"cards":common,"hands":hands,"graveyards":piles,"life":life,"game_over":self.state.outcome().is_some()},"all_cards":all,"all_hands":all_hands,"libraries_top_to_bottom":libraries,"graveyards_physical_top_to_bottom":physical_piles,"protocol":{"basis":basis(self.state.basis()),"executor_revision":execution.revision(),"registry_revision":registry.revision(),"active":seat(window.active_player()),"priority":self.holder().ok().map(seat),"window_phase":format!("{:?}",window.phase()),"position":turn_position_view(self.state.turn().expect("scheduled").position()),"stage":format!("{:?}",self.state.turn().expect("scheduled").stage()),"stack":stack,"stack_size":window.entries().len(),"pending":self.decision()?,"outcome":format!("{:?}",self.state.outcome()),"pending_eliminations":self.state.pending_eliminations().iter().map(|p|seat(*p)).collect::<Vec<_>>(),"mana":window.participants().iter().map(|p|(seat(*p),ManaType::ALL.iter().map(|t|(format!("{t:?}"),execution.mana(*p,*t))).collect::<BTreeMap<_,_>>())).collect::<BTreeMap<_,_>>()},"current_facts":self.current_facts}),
        )
    }
    fn facts(&self, candidate: &NativeCandidate, source: &GameObjects) -> TraceResult<Value> {
        let payment=candidate.mana_payment().map(|p|json!({"source":format!("{:?}",p.source),"payer":seat(p.payer),"cost":{"generic":p.cost.generic,"required":p.cost.required.iter().map(|(t,n)|(format!("{t:?}"),n.clone())).collect::<BTreeMap<_,_>>()},"spend":p.spend.iter().map(|s|json!({"type":format!("{:?}",s.mana_type),"amount":s.amount})).collect::<Vec<_>>()}));
        let production=candidate.mana_production().map(|p|json!({"source":format!("{:?}",p.source),"ability":spellwake_reference_tools::mana_ref::view(p.ability),"controller":seat(p.controller),"produced":p.produced.iter().map(|(t,n)|(format!("{t:?}"),n.clone())).collect::<BTreeMap<_,_>>()}));
        let activation=candidate.ability_activation().map(|a|json!({"entry":format!("{:?}",a.entry),"ability":a.ability.0,"controller":seat(a.controller),"source":check_game_object_view(&a.source,|id|self.label(id)),"source_cost":format!("{:?}",a.source_cost),"resulting_source":format!("{:?}",a.resulting_source)}));
        let draws=candidate.draw_journal().events.iter().map(|d|match d { DrawEvent::Drawn{player,before,after}=>json!({"kind":"drawn","player":seat(*player),"before":check_game_object_view(before,|id|self.label(id)),"after":check_game_object_view(after,|id|self.label(id))}),DrawEvent::EmptyLibrary{player,attempts}=>json!({"kind":"empty_library","player":seat(*player),"attempts":attempts}) }).collect::<Vec<_>>();
        let declarations=candidate.combat_declarations().iter().map(|d|match d { CombatDeclarationRecord::Attackers{player,origin,attackers,taps}=>json!({"kind":"attackers","player":seat(*player),"origin":format!("{origin:?}"),"selected":attackers.iter().map(|a|json!({"object":format!("{:?}",a.attacker),"defender":seat(a.defender)})).collect::<Vec<_>>(),"taps":taps.iter().map(|t|json!({"before":check_game_object_view(&t.before,|id|self.label(id)),"after":check_game_object_view(&t.after,|id|self.label(id))})).collect::<Vec<_>>()}),CombatDeclarationRecord::Blockers{player,origin,blockers}=>json!({"kind":"blockers","player":seat(*player),"origin":format!("{origin:?}"),"selected":blockers.iter().map(|b|json!({"blocker":format!("{:?}",b.blocker),"attacker":format!("{:?}",b.attacker)})).collect::<Vec<_>>()}) }).collect::<Vec<_>>();
        Ok(
            json!({"checks":check_journal_view(candidate.check_journal(),source,|id|self.label(id))?,"damage":damage_journal_view(candidate.damage_journal()),"mana_payment":payment,"mana_production":production,"ability_activation":activation,"draws":draws,"turn_transitions":candidate.turn_transitions().iter().map(|t|json!({"from":t.from.map(turn_position_view),"to":turn_position_view(t.to),"active_before":seat(t.active_before),"active_after":seat(t.active_after)})).collect::<Vec<_>>(),"combat_declarations":declarations,"combat_assignments":candidate.combat_assignments().iter().map(|a|json!({"player":seat(a.player),"origin":format!("{:?}",a.origin),"rows":a.assignments.iter().map(|r|json!({"source":format!("{:?}",r.source),"target":format!("{:?}",r.target),"amount":r.amount})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"land_play":candidate.land_play().map(|r|json!({"player":seat(r.player),"before":check_game_object_view(&r.before,|id|self.label(id)),"after":check_game_object_view(&r.after,|id|self.label(id))})),"permanent_entry":candidate.permanent_entry().map(|r|json!({"entry":format!("{:?}",r.entry),"before":check_game_object_view(&r.before,|id|self.label(id)),"after":check_game_object_view(&r.after,|id|self.label(id))})),"shuffle_count":candidate.shuffle_journal().len()}),
        )
    }
    fn command_view(&self, command: &GameCommand) -> TraceResult<Value> {
        Ok(match command {
            GameCommand::Pass => json!({"kind":"pass"}),
            GameCommand::ActivateMana { source, ability } => {
                json!({"kind":"activate_mana","source":self.alias(*source)?,"object":format!("{source:?}"),"ability":spellwake_reference_tools::mana_ref::view(*ability)})
            }
            GameCommand::ActivateAbility {
                source,
                ability,
                target,
                payment,
            } => {
                json!({"kind":"activate_ability","source":self.alias(*source)?,"object":format!("{source:?}"),"ability":ability.0,"target":self.target(*target)?,"payment":format!("{payment:?}")})
            }
            GameCommand::Cast { spell, target } => {
                json!({"kind":"cast","source":self.alias(*spell)?,"object":format!("{spell:?}"),"target":self.target(*target)?})
            }
            GameCommand::CastPaid {
                spell,
                target,
                payment,
            } => {
                json!({"kind":"cast_paid","source":self.alias(*spell)?,"object":format!("{spell:?}"),"target":self.target(*target)?,"payment":payment.spend.iter().map(|r|json!({"type":format!("{:?}",r.mana_type),"amount":r.amount})).collect::<Vec<_>>()})
            }
            GameCommand::Reply { decision, choice } => {
                json!({"kind":"reply","decision":format!("{decision:?}"),"choice":match choice {
                    DecisionReply::Discard(ids)=>json!({"kind":"discard","objects":ids.iter().map(|id|Ok(json!({"object":format!("{id:?}"),"alias":self.alias(*id)?}))).collect::<TraceResult<Vec<Value>>>()?}),
                    DecisionReply::OrderGraveyard(ids)=>json!({"kind":"order_graveyard","objects":ids.iter().map(|id|Ok(json!({"object":format!("{id:?}"),"alias":self.alias(*id)?}))).collect::<TraceResult<Vec<Value>>>()?}),
                    DecisionReply::DeclareAttackers(rows)=>json!({"kind":"declare_attackers","rows":rows.iter().map(|r|Ok(json!({"attacker":self.alias(r.attacker)?,"object":format!("{:?}",r.attacker),"defender":seat(r.defender)}))).collect::<TraceResult<Vec<Value>>>()?}),
                    DecisionReply::DeclareBlockers(rows)=>json!({"kind":"declare_blockers","rows":rows.iter().map(|r|Ok(json!({"blocker":self.alias(r.blocker)?,"attacker":self.alias(r.attacker)?,"blocker_object":format!("{:?}",r.blocker),"attacker_object":format!("{:?}",r.attacker)}))).collect::<TraceResult<Vec<Value>>>()?}),
                    DecisionReply::AssignCombatDamage(rows)=>json!({"kind":"assign_combat_damage","rows":rows.iter().map(|r|Ok(json!({"source":self.alias(r.source)?,"source_object":format!("{:?}",r.source),"target":self.recipient(r.target)?,"amount":r.amount}))).collect::<TraceResult<Vec<Value>>>()?}),
                    other=>return Err(format!("unplanned native reply {other:?}")),
                }})
            }
            other => return Err(format!("unplanned native command {other:?}")),
        })
    }
    fn act(&mut self, actor: SeatId, command: GameCommand) -> TraceResult<()> {
        let intent = self.command_view(&command)?;
        let before = self.snapshot()?;
        let before_basis = basis(self.state.basis());
        let text = format!("{command:?}");
        let candidate = spellwake_reference_tools::current_capture::advance(
            &self.state,
            GameInput {
                basis: self.state.basis().clone(),
                actor,
                command,
            },
        )
        .map_err(|e| format!("native {text}: {e:?}"))?;
        let facts = self.facts(&candidate, self.state.execution().window().cards())?;
        self.current_facts = facts.clone();
        self.state = candidate.into_state();
        self.commands.push(spellwake_reference_tools::capture_points::mark_value("Fixture::commands actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),&spellwake_reference_tools::packet::AliasContext::default(),json!({"ordinal":self.commands.len()+1,"actor":seat(actor),"before_basis":before_basis,"intent":intent,"debug_command":text,"before":before,"after":self.snapshot()?,"facts":facts})).expect("collect actual scoped native point"));
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
        for _ in 0..64 {
            if self.step() == step {
                return Ok(());
            }
            if self.state.pending_decision().is_some() {
                return Err(format!(
                    "unexpected prompt while approaching {step:?}: {:?}",
                    self.state.pending_decision()
                ));
            }
            self.pass()?;
        }
        Err(format!("did not reach {step:?}"))
    }
    fn settle_stack(&mut self) -> TraceResult<()> {
        for _ in 0..32 {
            if self.state.pending_decision().is_some() {
                return Err("unplanned prompt while resolving stack".into());
            }
            if self.state.execution().window().entries().is_empty() {
                return Ok(());
            }
            self.pass()?;
        }
        Err("stack did not settle".into())
    }
    fn row(&mut self, id: &str) -> TraceResult<()> {
        self.rows.push(
            spellwake_reference_tools::capture_points::mark_value(
                "Fixture::rows actual writer",
                spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),
                &spellwake_reference_tools::packet::AliasContext::default(),
                json!({"id":id,"comparison_scope":"common","snapshot":self.snapshot()?}),
            )
            .expect("collect actual scoped native point"),
        );
        Ok(())
    }
    fn native_row(&mut self, id: &str) -> TraceResult<()> {
        self.native_only_rows.push(spellwake_reference_tools::capture_points::mark_value("Fixture::native_only_rows actual writer",spellwake_reference_tools::current_profile::SnapshotOwner::GameState(&self.state),&spellwake_reference_tools::packet::AliasContext::default(),
            json!({"id":id,"comparison_scope":"native-only-protocol","snapshot":self.snapshot()?}),
        ).expect("collect actual scoped native point"));
        Ok(())
    }
    fn fund(&mut self, actor: SeatId, index: usize) -> TraceResult<()> {
        if self.holder()? != actor {
            return Err("funding actor lacks actual priority".into());
        }
        let source = self.object(&format!("fund-{}-{index}", seat(actor)))?;
        self.act(
            actor,
            GameCommand::ActivateMana {
                source,
                ability: spellwake_engine::mana::ManaAbilityRef::Printed(ManaAbilityId(1)),
            },
        )
    }
    fn reply_check(
        &mut self,
        actor: SeatId,
        domain: &[&str],
        selected: &[&str],
        discard: bool,
    ) -> TraceResult<()> {
        let p = self
            .state
            .pending_decision()
            .ok_or("missing actual check prompt")?;
        let RuleFrame::Checks(frame) = p.frame() else {
            return Err("expected actual check frame".into());
        };
        if p.chooser() != actor
            || frame.chooser() != actor
            || matches!(frame, CheckDecision::CleanupDiscard(_)) != discard
        {
            return Err("wrong check actor/kind".into());
        }
        let actual_domain = match frame {
            CheckDecision::CleanupDiscard(f) if discard => {
                if f.required() != selected.len() {
                    return Err("wrong actual required discard count".into());
                }
                f.domain()
            }
            CheckDecision::GraveyardOrder(f) if !discard => f.domain(),
            _ => return Err("wrong check choice family".into()),
        };
        self.require_domain(actual_domain, domain)?;
        if !discard && frame.boundary() != CheckBoundary::Priority {
            return Err("unexpected order parent boundary".into());
        }
        let decision = p.id();
        let rows = selected
            .iter()
            .map(|alias| self.object(alias))
            .collect::<TraceResult<Vec<_>>>()?;
        if rows.iter().copied().collect::<BTreeSet<_>>().len() != rows.len()
            || rows.iter().any(|id| !actual_domain.contains(id))
        {
            return Err("authored order does not address exact current domain".into());
        }
        self.act(
            actor,
            GameCommand::Reply {
                decision,
                choice: if discard {
                    DecisionReply::Discard(rows)
                } else {
                    DecisionReply::OrderGraveyard(rows)
                },
            },
        )
    }
    fn attack(
        &mut self,
        actor: SeatId,
        eligible: &[&str],
        selection: &[(&str, SeatId)],
    ) -> TraceResult<()> {
        let p = self
            .state
            .pending_decision()
            .ok_or("missing actual attacker prompt")?;
        let RuleFrame::Combat(CombatDecision::Attackers(f)) = p.frame() else {
            return Err("unexpected attacker frame".into());
        };
        if p.chooser() != actor {
            return Err("wrong attacking actor".into());
        }
        self.require_domain(f.eligible_attackers(), eligible)?;
        let decision = p.id();
        let rows = selection
            .iter()
            .map(|(alias, defender)| {
                Ok(AttackDeclaration {
                    attacker: self.object(alias)?,
                    defender: *defender,
                    defending_planeswalker: None,
                })
            })
            .collect::<TraceResult<Vec<_>>>()?;
        self.act(
            actor,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::DeclareAttackers(rows),
            },
        )
    }
    fn block(
        &mut self,
        actor: SeatId,
        eligible: &[&str],
        attackers: &[&str],
        selection: &[(&str, &str)],
    ) -> TraceResult<()> {
        let p = self
            .state
            .pending_decision()
            .ok_or("missing actual blocker prompt")?;
        let RuleFrame::Combat(CombatDecision::Blockers(f)) = p.frame() else {
            return Err("unexpected blocker frame".into());
        };
        if p.chooser() != actor {
            return Err("wrong blocking actor".into());
        }
        self.require_domain(f.eligible_blockers(), eligible)?;
        self.require_domain(f.attackers(), attackers)?;
        let decision = p.id();
        let rows = selection
            .iter()
            .map(|(blocker, attacker)| {
                Ok(BlockDeclaration {
                    blocker: self.object(blocker)?,
                    attacker: self.object(attacker)?,
                })
            })
            .collect::<TraceResult<Vec<_>>>()?;
        self.act(
            actor,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::DeclareBlockers(rows),
            },
        )
    }
    fn assign_split(&mut self) -> TraceResult<()> {
        let p = self
            .state
            .pending_decision()
            .ok_or("missing actual split damage prompt")?;
        let RuleFrame::Combat(CombatDecision::DamageAssignments(f)) = p.frame() else {
            return Err("unexpected assignment frame".into());
        };
        if p.chooser() != A || f.domains().len() != 1 {
            return Err("unexpected split actor/domain count".into());
        }
        let domain = &f.domains()[0];
        if domain.source() != self.object("a1")?
            || domain.power() != &6.into()
            || domain.recipients().iter().copied().collect::<BTreeSet<_>>()
                != [
                    DamageRecipient::Creature(self.object("b1")?),
                    DamageRecipient::Creature(self.object("b2")?),
                ]
                .into_iter()
                .collect()
        {
            return Err("unexpected split domain".into());
        }
        let decision = p.id();
        let source = domain.source();
        let rows = ["b1", "b2"]
            .iter()
            .map(|alias| {
                Ok(CombatDamageAssignment {
                    source,
                    target: DamageRecipient::Creature(self.object(alias)?),
                    amount: 3.into(),
                })
            })
            .collect::<TraceResult<Vec<_>>>()?;
        self.native_row("split-assignment-pending")?;
        self.act(
            A,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::AssignCombatDamage(rows),
            },
        )
    }
    fn finish(self, case: &str) -> TraceResult<Value> {
        if self.state.pending_decision().is_some() {
            return Err(format!("{case} ended with unconsumed prompt"));
        }
        Ok(
            json!({"id":case,"mode":if case=="C05"{"native-root-prepared-Cleanup"}else{"native-root-prepared-scheduled-position"},"seed":self.seed,"initialization":self.initialization,"rows":self.rows,"native_only_rows":self.native_only_rows,"commands":self.commands,"final_basis":basis(self.state.basis())}),
        )
    }
}

fn run_root(case: &str) -> TraceResult<Value> {
    let mut f = Fixture::new(case)?;
    match case {
        "C01" => {
            f.round()?;
            f.row("C01-discard-pending")?;
            f.reply_check(
                A,
                &["h0", "h1", "h2", "h3", "h4", "h5", "h6", "h7", "h8", "h9"],
                &["h8", "h2", "h9"],
                true,
            )?;
            f.row("C01-after-cleanup")?;
        }
        "C02" | "C03" => {
            f.row(&format!("{case}-before-cleanup"))?;
            f.round()?;
            f.row(&format!("{case}-after-cleanup"))?;
        }
        "C04" => {
            f.pass()?;
            f.fund(B, 0)?;
            f.act(
                B,
                GameCommand::ActivateAbility {
                    source: f.object("pyrite")?,
                    ability: ActivatedAbilityId(1),
                    target: TargetRef::Object(f.object("giant")?),
                    payment: None,
                },
            )?;
            f.row("C04-cost-paid")?;
            f.settle_stack()?;
            f.round()?;
            f.row("C04-discard-pending")?;
            f.reply_check(
                A,
                &["h0", "h1", "h2", "h3", "h4", "h5", "h6", "h7", "h8"],
                &["h7", "h2"],
                true,
            )?;
            f.row("C04-after-cleanup")?;
        }
        "C05" => {
            f.row("C05-first-discard-pending")?;
            f.reply_check(
                A,
                &["h0", "h1", "h2", "h3", "h4", "h5", "h6", "h7", "h8"],
                &["h8", "h1"],
                true,
            )?;
            f.row("C05-sba-exception")?;
            f.pass()?;
            f.fund(B, 0)?;
            f.act(
                B,
                GameCommand::Cast {
                    spell: f.object("response")?,
                    target: TargetRef::Object(f.object("returnee")?),
                },
            )?;
            f.row("C05-bounce-admitted")?;
            f.settle_stack()?;
            f.row("C05-bounce-resolved")?;
            f.round()?;
            f.row("C05-second-discard-pending")?;
            f.reply_check(
                A,
                &["h0", "h2", "h3", "h4", "h5", "h6", "h7", "returnee"],
                &["returnee"],
                true,
            )?;
            f.row("C05-after-repeat")?;
        }
        "C06" | "C07" => {
            f.go(Step::DeclareAttackers)?;
            f.attack(A, &["a1"], &[("a1", B)])?;
            f.go(Step::DeclareBlockers)?;
            f.block(B, &["b1", "b2"], &["a1"], &[("b1", "a1"), ("b2", "a1")])?;
            f.go(Step::CombatDamage)?;
            f.assign_split()?;
            f.row(&format!("{case}-order-pending"))?;
            f.reply_check(
                B,
                &["b1", "b2"],
                if case == "C06" {
                    &["b1", "b2"]
                } else {
                    &["b2", "b1"]
                },
                false,
            )?;
            f.row(&format!("{case}-after-sba"))?;
        }
        "C08" => {
            f.go(Step::DeclareAttackers)?;
            f.attack(C, &["a1", "a2", "c1"], &[("a1", B), ("a2", B), ("c1", A)])?;
            f.go(Step::DeclareBlockers)?;
            f.block(
                B,
                &["b1", "b2"],
                &["a1", "a2"],
                &[("b1", "a1"), ("b2", "a2")],
            )?;
            f.block(A, &["c2"], &["c1"], &[("c2", "c1")])?;
            f.go(Step::CombatDamage)?;
            f.row("C08-order-C-pending")?;
            f.reply_check(C, &["c1", "c2"], &["c2", "c1"], false)?;
            f.row("C08-order-B-pending")?;
            f.reply_check(B, &["b1", "b2"], &["b1", "b2"], false)?;
            f.row("C08-order-A-pending")?;
            f.reply_check(A, &["a1", "a2"], &["a2", "a1"], false)?;
            f.row("C08-after-sba")?;
        }
        "C09" => {
            for index in 0..3 {
                f.fund(A, index)?;
            }
            f.act(
                A,
                GameCommand::Cast {
                    spell: f.object("response")?,
                    target: TargetRef::Object(f.object("giant")?),
                },
            )?;
            let target = f
                .state
                .execution()
                .window()
                .entries()
                .last()
                .ok_or("no admitted response")?
                .id();
            f.row("C09-target-spell-admitted")?;
            f.act(
                A,
                GameCommand::Cast {
                    spell: f.object("counter")?,
                    target: TargetRef::Stack(target),
                },
            )?;
            f.row("C09-counter-admitted")?;
            f.settle_stack()?;
            f.row("C09-after-counter")?;
        }
        "C10" => {
            for index in 0..3 {
                f.fund(A, index)?;
            }
            f.act(
                A,
                GameCommand::CastPaid {
                    spell: f.object("source")?,
                    target: TargetRef::Object(f.object("giant")?),
                    payment: ManaPayment {
                        from_lots: Default::default(),
                        spend: vec![ManaSpend {
                            mana_type: ManaType::Red,
                            amount: 3.into(),
                        }],
                    },
                },
            )?;
            f.row("C10-source-admitted")?;
            f.settle_stack()?;
            f.row("C10-after-sba")?;
        }
        "C11" => {
            let target = f.object("giant")?;
            f.fund(A, 0)?;
            f.act(
                A,
                GameCommand::ActivateAbility {
                    source: f.object("pyrite1")?,
                    ability: ActivatedAbilityId(1),
                    target: TargetRef::Object(target),
                    payment: None,
                },
            )?;
            f.row("C11-first-cost-paid")?;
            f.settle_stack()?;
            f.row("C11-first-effect-settled")?;
            f.fund(A, 1)?;
            f.act(
                A,
                GameCommand::ActivateAbility {
                    source: f.object("pyrite2")?,
                    ability: ActivatedAbilityId(1),
                    target: TargetRef::Object(target),
                    payment: None,
                },
            )?;
            f.row("C11-second-cost-paid")?;
            f.settle_stack()?;
            f.row("C11-after-sba")?;
        }
        _ => return Err("unknown native root script".into()),
    }
    f.finish(case)
}

fn primitive() -> TraceResult<Value> {
    let mut cards = spellwake_reference_tools::current_capture::card_new(0, &[A, B]);
    let mut identities = BTreeMap::new();
    let mut seeds = vec![];
    for (index, (alias, name, owner)) in [
        ("A-old-bottom", "Island", A),
        ("traveller", "Hill Giant", A),
        ("A-old-top", "Plains", A),
        ("B-old-bottom", "Island", B),
        ("B-old-top", "Plains", B),
    ]
    .into_iter()
    .enumerate()
    {
        let card = CardId(index as u64 + 1);
        let change = spellwake_reference_tools::current_capture::card_seed_card(
            &cards,
            cards.revision(),
            card,
            owner,
            ObjectDestination::Graveyard,
            &[A, B],
        )
        .map_err(|e| format!("{e:?}"))?;
        cards = change.next;
        identities.insert(card, (alias, name));
        seeds.push(json!({"alias":alias,"name":name,"owner":seat(owner),"object":format!("{:?}",change.after.id())}));
    }
    let snapshot = |registry: &GameObjects| -> TraceResult<Value> {
        let mut physical = serde_json::Map::new();
        let mut hands = BTreeMap::new();
        let mut piles = BTreeMap::new();
        for object in registry.iter() {
            let (alias, name) =
                identities[&object.card_id().expect("source-authored card-only trace")];
            physical.insert(alias.into(),json!({"name":name,"owner":seat(object.owner()),"zone":zone(object.zone()),"controller":object.controller().map(seat),"tapped":object.tapped().unwrap_or(false),"power":Value::Null,"toughness":Value::Null,"marked_damage":Value::Null,"physical":check_game_object_view(object,|id|identities[&id].0.into())}));
        }
        for player in [A, B] {
            hands.insert(
                seat(player),
                registry
                    .iter()
                    .filter(|c| c.zone() == ObjectZone::Hand(player))
                    .map(|c| identities[&c.card_id().expect("source-authored card-only trace")].0)
                    .collect::<Vec<_>>(),
            );
            piles.insert(
                seat(player),
                registry
                    .graveyard(player)
                    .iter()
                    .map(|id| {
                        registry
                            .get(*id)
                            .map(|c| {
                                identities[&c.card_id().expect("source-authored card-only trace")].0
                            })
                            .ok_or("missing primitive pile object".into())
                    })
                    .collect::<TraceResult<Vec<_>>>()?,
            );
        }
        Ok(
            json!({"registry_revision":registry.revision(),"state":{"cards":physical,"hands":hands,"graveyards":piles,"life":{"A":"20","B":"20"},"game_over":false},"life_scope":"literal fixture metadata, no native life owner in GameObjects"}),
        )
    };
    let initial = snapshot(&cards)?;
    let initial_point = spellwake_reference_tools::capture_points::token(
        "C12 initial physical row",
        spellwake_reference_tools::current_profile::SnapshotOwner::GameObjects {
            cards: &cards,
            participants: &[A, B],
        },
        &spellwake_reference_tools::packet::AliasContext::default(),
    )?;
    let original = cards
        .current_identity(CardId(2))
        .ok_or("traveller missing")?;
    let departed = spellwake_reference_tools::current_capture::card_move(
        &cards,
        cards.revision(),
        original,
        ObjectDestination::Hand,
        &[A, B],
    )
    .and_then(spellwake_engine::object_identity::ZoneMoveResult::require_moved)
    .map_err(|e| format!("{e:?}"))?;
    let departure = json!({"before":check_game_object_view(departed.before.as_ref().ok_or("missing departure before")?,|id|identities[&id].0.into()),"after":check_game_object_view(&departed.after,|id|identities[&id].0.into()),"prior_exact_object_absent":departed.next.get(original).is_none()});
    let middle = snapshot(&departed.next)?;
    let middle_point = spellwake_reference_tools::capture_points::token(
        "C12 departed physical row",
        spellwake_reference_tools::current_profile::SnapshotOwner::GameObjects {
            cards: &departed.next,
            participants: &[A, B],
        },
        &spellwake_reference_tools::packet::AliasContext::default(),
    )?;
    let reentered = spellwake_reference_tools::current_capture::card_move(
        &departed.next,
        departed.next.revision(),
        departed.after.id(),
        ObjectDestination::Graveyard,
        &[A, B],
    )
    .and_then(spellwake_engine::object_identity::ZoneMoveResult::require_moved)
    .map_err(|e| format!("{e:?}"))?;
    let reentry = json!({"before":check_game_object_view(reentered.before.as_ref().ok_or("missing reentry before")?,|id|identities[&id].0.into()),"after":check_game_object_view(&reentered.after,|id|identities[&id].0.into()),"prior_exact_object_absent":reentered.next.get(departed.after.id()).is_none(),"initial_exact_object_absent":reentered.next.get(original).is_none()});
    Ok(
        json!({"id":"C12","mode":"native-GameObjects-trusted-primitives","seed_chronology":seeds,"rows":[primitive_row("C12-initial",initial,initial_point)?,primitive_row("C12-departed",middle,middle_point)?,spellwake_reference_tools::capture_points::mark_value("C12 reentered physical row",spellwake_reference_tools::current_profile::SnapshotOwner::GameObjects{cards:&reentered.next,participants:&[A,B]},&spellwake_reference_tools::packet::AliasContext::default(),json!({"id":"C12-reentered","comparison_scope":"common-primitive-only","snapshot":snapshot(&reentered.next)?}))?],"movements":[departure,reentry],"root_commands":[],"limitations":"not GameState, no player movement permission or numeric cross-engine identity equivalence"}),
    )
}

fn main() -> Result<(), String> {
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
        "cleanup",
        &[spellwake_reference_tools::current_capture::repository().join(file!())],
        None,
    )?;
    let mut cases = vec![];
    for id in [
        "C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11",
    ] {
        cases.push(run_root(id)?);
    }
    cases.push(primitive()?);
    let mut observations = serde_json::json!({"schema_version":1,"kind":"actual-native-cleanup-discard-trace","reads_expected_or_reference_input":false,"root_cases":11,"primitive_cases":1,"reference_only_P01_included":false,"rules":"CR20260925","qualification":"literal bounded native card programs, not complete card/game/format qualification","orientation":"native physical graveyards top-to-bottom; no sorting or normalization","protocol_differences":{"initial_positions":"C01-C04 End; C05 Cleanup; C06-C08 actual Untap; C09-C11 PrecombatMain","control_age":"actual native controller-turn-start hook for combat only","funding":"actual basic-land ActivateMana commands rather than Forge floating-credit protocol","libraries":"64 explicit Island aliases per seat; actual draws fully retained in all_cards/all_hands and current draw facts","common_hands":"tracked case aliases only; actual auxiliary membership separately retained","nonbattlefield_tapped":"common false alongside full physical tapped:null snapshot","C05":"explicit authored persistent 3/0 override at prepared Cleanup","C12":"trusted GameObjects primitive, no root or native life comparison"},"cases":cases});
    spellwake_reference_tools::capture_points::bind_value_points(
        &mut observations,
        "/observations",
    )?;
    let envelope = spellwake_reference_tools::current_capture::finish("cleanup", observations)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&envelope).expect("serialize actual current capture")
    );
    Ok(())
}

fn primitive_row(id: &str, snapshot: Value, point: Option<String>) -> TraceResult<Value> {
    let mut row = json!({"id":id,"comparison_scope":"common-primitive-only","snapshot":snapshot});
    if let Some(point) = point {
        row["__current_capture_point"] = json!(point);
    }
    Ok(row)
}
