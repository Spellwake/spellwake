//! Source-backed opening/resource prefixes, not complete games or card qualification.
#[path = "support/natural_selection_source.rs"]
mod natural_selection_source;
use natural_selection_source::{digest, inputs, read_input};
use serde_json::{Value, json};
use spellwake_card_programs::{
    OracleId, RESOLUTION_SELECTION_NATURAL_MANA_PROFILE, Registry, TEACHING_SOURCE_SHA256,
};
use spellwake_contracts::names::NameRules;
use spellwake_engine::{
    combat::{CombatDeclarationRecord, CombatRules, DeclarationOrigin},
    draw::DrawEvent,
    game::{
        DecisionReply, GameCommand, GameContext, GameError, GameId, GameInput, GameState,
        NativeCandidate, OpeningSetup, RuleFrame,
    },
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaCost, ManaPayment, ManaSpend, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId, ObjectOrigin, ObjectZone},
    occurrence::{EntryCompletion, EventCause, RulesOccurrence, RulesRecord, ZoneReason},
    opening::{MulliganChoice, OpeningDecision},
    priority::{PriorityPhase, SeatId},
    program::{Instruction, ProgramKind},
    setup::GameSeed,
    shuffle::{EntropyTape, SHUFFLE_ALGORITHM},
    turn::Step,
};
use spellwake_reference_tools::packet;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
const SWAMP: &str = "56719f6a-1a6c-4c0a-8d21-18f7d7350b68";
const FOREST: &str = "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6";
const PLAINS: &str = "bc71ebf6-2056-41f7-be35-b2e5c34afa99";
const ISLAND: &str = "b2c6aa39-2d2a-459c-a555-fb48ba993373";
const BEARS: &str = "14c8f55d-d177-4c25-a931-ebeb9e6062a0";
const RAISE: &str = "5b2364d7-a811-4595-a1b4-224c70555ffa";
const EDICT: &str = "058917c1-21ab-488a-9f9c-591c55f3c596";
const GETH: &str = "0f575f18-4606-4b85-9a90-07f8d8d46d06";
const FLESHBAG: &str = "4b1bf05e-753e-4350-a913-894cf3cecc0c";

struct Deck {
    owner: SeatId,
    label: String,
    oracle_order: Vec<String>,
    physical_order: Vec<CardId>,
}
fn declared_order(declaration: &Value) -> Vec<String> {
    let label = declaration["label"].as_str().unwrap();
    let (partner, spell) = match label {
        "BG" => (FOREST, BEARS),
        "BW" => (PLAINS, RAISE),
        _ => panic!("unlisted retained deck"),
    };
    let rows = declaration["main_deck"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    let mut remaining = BTreeMap::new();
    for row in rows {
        assert!(
            remaining
                .insert(
                    row["oracle_id"].as_str().unwrap(),
                    usize::try_from(row["count"].as_u64().unwrap()).unwrap()
                )
                .is_none()
        );
    }
    assert_eq!(remaining.values().sum::<usize>(), 60);
    assert_eq!(remaining[SWAMP], 20);
    assert_eq!(remaining[partner], 8);
    assert_eq!(remaining[ISLAND], 4);
    assert_eq!(remaining.values().filter(|&&count| count == 4).count(), 8);
    // Disclosed test order only: seven named source identities first, then all
    // remaining copies in the original retained declaration order. No card is replaced.
    let mut order = Vec::new();
    for oracle in [SWAMP, partner, spell, ISLAND, EDICT, GETH, FLESHBAG] {
        *remaining.get_mut(oracle).unwrap() -= 1;
        order.push(oracle.to_owned());
    }
    for row in rows {
        let oracle = row["oracle_id"].as_str().unwrap();
        order.extend(std::iter::repeat_n(oracle.to_owned(), remaining[oracle]));
    }
    assert_eq!(order.len(), 60);
    let actual = order.iter().fold(BTreeMap::new(), |mut counts, id| {
        *counts.entry(id.as_str()).or_insert(0usize) += 1;
        counts
    });
    for row in rows {
        assert_eq!(
            actual[row["oracle_id"].as_str().unwrap()],
            usize::try_from(row["count"].as_u64().unwrap()).unwrap()
        );
    }
    order
}

struct SavedCall {
    input: GameInput,
    result: Result<NativeCandidate, GameError>,
}
struct Prefix {
    setup: OpeningSetup,
    context: GameContext,
    game: GameId,
    initialization: Result<NativeCandidate, GameError>,
    state: GameState,
    calls: Vec<SavedCall>,
    decks: Vec<Deck>,
}
impl Prefix {
    fn new(starter: &str) -> Self {
        let list_bytes = read_input("SPELLWAKE_SELECTION_NATURAL_DECKS_INPUT");
        assert_eq!(
            digest(&list_bytes),
            "6c6d436357aa8b1732185b29f25e3f1354d08e6083e963f8771c8c975bcd8265"
        );
        let lists = packet::unique_json(&list_bytes).unwrap();
        assert_eq!(
            lists["proposed_profile"],
            RESOLUTION_SELECTION_NATURAL_MANA_PROFILE
        );
        assert_eq!(lists["source_union_count"], 12);
        let (inventory, vocabulary, _) = inputs();
        let registry =
            Registry::from_resolution_selection_natural_mana_inventory(&inventory, &vocabulary)
                .unwrap();
        assert_eq!(registry.coverage().registry_version, 10);
        assert_eq!(registry.coverage().required_partial_bindings, 12);
        assert_eq!(registry.coverage().fully_qualified_cards, 0);
        let vocabulary = Arc::new(vocabulary);
        let names = Arc::new(NameRules::literal_names());
        let context = GameContext::new(
            "natural-resource-prefix-v1".into(),
            vocabulary.rules_revision().into(),
            TEACHING_SOURCE_SHA256.into(),
            RESOLUTION_SELECTION_NATURAL_MANA_PROFILE.into(),
            "native-typed-prefix".into(),
            vocabulary.id(),
            names.id(),
        )
        .unwrap();
        let labels = match starter {
            "BG" => ["BG", "BW"],
            "BW" => ["BW", "BG"],
            _ => panic!("unlisted prefix"),
        };
        let mut cards = Vec::new();
        let mut decks = Vec::new();
        for (owner, label, base) in [(A, labels[0], 1000u64), (B, labels[1], 2000u64)] {
            let declaration = lists["decks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["label"] == label)
                .unwrap();
            let oracle_order = declared_order(declaration);
            let mut physical_order = Vec::new();
            for (index, oracle) in oracle_order.iter().enumerate() {
                let physical = CardId(base + u64::try_from(index).unwrap() + 1);
                let identity = OracleId::try_from(oracle.clone()).unwrap();
                cards.push(
                    registry
                        .seed(&identity, physical, owner, ObjectDestination::LibraryBottom)
                        .unwrap(),
                );
                physical_order.push(physical);
            }
            decks.push(Deck {
                owner,
                label: label.into(),
                oracle_order,
                physical_order,
            });
        }
        assert_eq!(cards.len(), 120);
        let union: BTreeSet<_> = decks
            .iter()
            .flat_map(|deck| deck.oracle_order.iter().cloned())
            .collect();
        assert_eq!(
            union,
            lists["source_union"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect()
        );
        // Explicit identity Fisher-Yates transcript: selected=index at every
        // descending swap, for both60-card libraries. This is not random entropy.
        let words: Vec<u64> = (1..60).rev().chain((1..60).rev()).collect();
        assert_eq!(words.len(), 118);
        println!(
            "{}",
            json!({"kind":"disclosed-native-prefix-inputs","starter":starter,"decks":decks.iter().map(|deck|json!({"owner":deck.owner.0,"label":deck.label,"physical_top_to_bottom":deck.physical_order.iter().map(|id|id.0).collect::<Vec<_>>(),"oracle_top_to_bottom":deck.oracle_order})).collect::<Vec<_>>(),"shuffle_algorithm":SHUFFLE_ALGORITHM,"entropy_words":words,"starting_life":20,"starting_mana":[],"starting_battlefield":[]})
        );
        let seed = GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocabulary,
            names,
        );
        assert!(seed.mana.is_empty());
        assert!(seed.initial_battlefield_order.is_empty());
        assert!(
            seed.cards
                .iter()
                .all(|card| card.destination == ObjectDestination::LibraryBottom)
        );
        let setup = OpeningSetup {
            seed,
            entropy: EntropyTape::new(words),
        };
        let game = GameId(if starter == "BG" { 49001 } else { 49002 });
        let initialization = GameState::initialize_opening(game, context.clone(), setup.clone());
        let actual = initialization.as_ref().unwrap();
        assert_eq!(actual.shuffle_journal().len(), 2);
        for (index, record) in actual.shuffle_journal().iter().enumerate() {
            assert_eq!(record.player, decks[index].owner);
            assert_eq!(record.algorithm, SHUFFLE_ALGORITHM);
            assert_eq!(record.before, record.after);
            assert_eq!(record.before.len(), 60);
            assert_eq!(record.entropy_start, index * 59);
            assert_eq!(record.entropy_end, (index + 1) * 59);
            let physical: Vec<_> = record
                .after
                .iter()
                .map(|id| {
                    actual
                        .state()
                        .execution()
                        .window()
                        .cards()
                        .get(*id)
                        .map(|o| o.card_id().unwrap())
                        .or_else(|| {
                            actual
                                .draw_journal()
                                .events
                                .iter()
                                .find_map(|event| match event {
                                    DrawEvent::Drawn { before, .. } if before.id() == *id => {
                                        before.card_id()
                                    }
                                    _ => None,
                                })
                        })
                        .unwrap()
                })
                .collect();
            assert_eq!(physical, decks[index].physical_order);
        }
        assert_eq!(actual.draw_journal().events.len(), 14);
        for (index, event) in actual.draw_journal().events.iter().enumerate() {
            let DrawEvent::Drawn {
                player,
                before,
                after,
            } = event
            else {
                panic!("actual full opening draw")
            };
            let deck = &decks[index / 7];
            assert_eq!(*player, deck.owner);
            assert_eq!(before.card_id(), Some(deck.physical_order[index % 7]));
            assert_eq!(after.card_id(), before.card_id());
            assert_ne!(before.id(), after.id());
            assert_eq!(before.zone(), ObjectZone::Library(*player));
            assert_eq!(after.zone(), ObjectZone::Hand(*player));
        }
        let state = actual.state().clone();
        for deck in &decks {
            assert_eq!(
                state.execution().window().cards().library(deck.owner).len(),
                53
            );
        }
        let result = Self {
            setup,
            context,
            game,
            initialization,
            state,
            calls: Vec::new(),
            decks,
        };
        result.no_mana();
        result
    }
    fn record(&mut self, actor: SeatId, command: GameCommand) -> NativeCandidate {
        let before = self.state.clone();
        let input = GameInput {
            basis: before.basis().clone(),
            actor,
            command,
        };
        let result = self.state.advance(input.clone());
        assert_eq!(self.state, before, "borrowed native source unchanged");
        self.calls.push(SavedCall {
            input,
            result: result.clone(),
        });
        let actual = result.unwrap();
        self.state = actual.state().clone();
        actual
    }
    fn ready(&self, active: SeatId, number: u64, step: Step) {
        assert_eq!(self.state.turn().unwrap().position().number, number);
        assert_eq!(self.state.turn().unwrap().position().step, step);
        assert_eq!(self.state.execution().window().active_player(), active);
        assert_eq!(
            self.state.execution().window().phase(),
            WindowPhase::Priority(PriorityPhase::Ready { holder: active })
        );
        assert!(self.state.pending_decision().is_none());
    }
    fn round(&mut self, active: SeatId) -> NativeCandidate {
        let other = if active == A { B } else { A };
        self.record(active, GameCommand::Pass);
        self.record(other, GameCommand::Pass)
    }
    fn keep(&mut self, actor: SeatId) {
        let pending = self.state.pending_decision().unwrap();
        assert_eq!(pending.chooser(), actor);
        assert_eq!(
            pending.frame(),
            &RuleFrame::Opening(OpeningDecision::Declare {
                player: actor,
                taken: 0,
                can_mulligan: true
            })
        );
        self.record(
            actor,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::Mulligan(MulliganChoice::Keep),
            },
        );
    }
    fn advance_from_upkeep_to_main(&mut self, active: SeatId, number: u64, draw: bool) {
        self.ready(active, number, Step::Upkeep);
        let transition = self.round(active);
        if draw {
            self.ready(active, number, Step::Draw);
            let [
                DrawEvent::Drawn {
                    player,
                    before,
                    after,
                },
            ] = transition.draw_journal().events.as_slice()
            else {
                panic!("one actual turn draw")
            };
            assert_eq!(*player, active);
            assert_eq!(before.card_id(), after.card_id());
            assert_ne!(before.id(), after.id());
            assert_eq!(before.zone(), ObjectZone::Library(active));
            assert_eq!(after.zone(), ObjectZone::Hand(active));
            self.round(active);
        } else {
            assert!(transition.draw_journal().events.is_empty());
        }
        self.ready(active, number, Step::PrecombatMain);
    }
    fn finish_empty_turn(&mut self, active: SeatId, number: u64) {
        self.ready(active, number, Step::PrecombatMain);
        self.round(active);
        self.ready(active, number, Step::BeginningCombat);
        let attackers = self.round(active);
        self.ready(active, number, Step::DeclareAttackers);
        assert!(attackers.combat_declarations().iter().any(|row|matches!(row,CombatDeclarationRecord::Attackers{origin:DeclarationOrigin::ForcedEmpty,attackers,..} if attackers.is_empty())));
        self.round(active);
        self.ready(active, number, Step::EndCombat);
        self.round(active);
        self.ready(active, number, Step::PostcombatMain);
        self.round(active);
        self.ready(active, number, Step::End);
        self.round(active);
        self.ready(if active == A { B } else { A }, number + 1, Step::Upkeep);
    }
    fn object(&self, card: CardId) -> ObjectId {
        self.state
            .execution()
            .window()
            .cards()
            .current_identity(card)
            .unwrap()
    }
    fn land(&mut self, actor: SeatId, card: CardId) {
        let before = self.object(card);
        assert_eq!(
            self.state
                .execution()
                .window()
                .cards()
                .get(before)
                .unwrap()
                .zone(),
            ObjectZone::Hand(actor)
        );
        let actual = self.record(actor, GameCommand::PlayLand { land: before });
        let entry = actual.land_play().unwrap();
        assert_eq!(entry.player, actor);
        assert_eq!(entry.before.id(), before);
        assert_eq!(entry.after.card_id(), Some(card));
        assert_ne!(entry.before.id(), entry.after.id());
        assert_eq!(entry.after.zone(), ObjectZone::Battlefield);
        assert!(actual.rules_journal().records.iter().any(|row|matches!(row,RulesRecord::Event(event) if event.occurrences.iter().any(|occurrence|matches!(&occurrence.value,RulesOccurrence::ZoneChange(change) if change.reason==ZoneReason::LandPlay && change.before.object.id()==before && change.after.object.id()==entry.after.id())))));
    }
    fn no_mana(&self) {
        for player in [A, B] {
            for kind in ManaType::ALL {
                assert_eq!(self.state.execution().mana(player, kind), 0.into());
            }
        }
    }
    fn mana(&mut self, card: CardId, land: BasicLandType, kind: ManaType) {
        let source = self.object(card);
        assert!(
            self.state
                .execution()
                .base_program(source)
                .unwrap()
                .mana
                .is_empty()
        );
        let actual = self.record(
            A,
            GameCommand::ActivateMana {
                source,
                ability: ManaAbilityRef::BasicLand(land),
            },
        );
        let produced = actual.mana_production().unwrap();
        assert_eq!(produced.source, source);
        assert_eq!(produced.controller, A);
        assert_eq!(produced.ability, ManaAbilityRef::BasicLand(land));
        assert_eq!(produced.produced, BTreeMap::from([(kind, 1.into())]));
        assert_eq!(
            self.state
                .execution()
                .window()
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
    fn fund_and_cast(&mut self, color: ManaType, land: BasicLandType) -> NativeCandidate {
        self.keep(A);
        self.keep(B);
        assert!(self.state.opening().unwrap().complete());
        for player in [A, B] {
            assert!(self.state.opening().unwrap().kept(player));
            assert_eq!(self.state.execution().life_total(player), Some(&20.into()));
        }
        assert!(
            self.state
                .execution()
                .window()
                .cards()
                .iter()
                .all(|o| o.zone() != ObjectZone::Battlefield)
        );
        self.advance_from_upkeep_to_main(A, 1, false);
        self.land(A, CardId(1001));
        self.finish_empty_turn(A, 1);
        self.advance_from_upkeep_to_main(B, 2, true);
        self.land(B, CardId(2004));
        self.finish_empty_turn(B, 2);
        self.advance_from_upkeep_to_main(A, 3, true);
        self.land(A, CardId(1002));
        self.no_mana();
        self.mana(CardId(1001), BasicLandType::Swamp, ManaType::Black);
        assert_eq!(self.state.execution().mana(A, ManaType::Black), 1.into());
        assert_eq!(self.state.execution().mana(A, color), 0.into());
        self.mana(CardId(1002), land, color);
        assert_eq!(self.state.execution().mana(A, ManaType::Black), 1.into());
        assert_eq!(self.state.execution().mana(A, color), 1.into());
        let spell = self.object(CardId(1003));
        let actual = self.record(
            A,
            GameCommand::CastUntargeted {
                spell,
                payment: Some(ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![
                        ManaSpend {
                            mana_type: ManaType::Black,
                            amount: 1.into(),
                        },
                        ManaSpend {
                            mana_type: color,
                            amount: 1.into(),
                        },
                    ],
                }),
            },
        );
        let paid = actual.mana_payment().unwrap();
        assert_eq!(paid.payer, A);
        assert_eq!(
            paid.cost,
            ManaCost {
                generic: 1.into(),
                required: BTreeMap::from([(color, 1.into())])
            }
        );
        assert_eq!(
            paid.spend
                .iter()
                .map(|row| (row.mana_type, row.amount.clone()))
                .collect::<BTreeMap<_, _>>(),
            BTreeMap::from([(ManaType::Black, 1.into()), (color, 1.into())])
        );
        self.no_mana();
        assert_eq!(self.state.execution().window().entries().len(), 1);
        assert_eq!(
            self.state.execution().window().entries()[0].id(),
            paid.source
        );
        actual
    }
    fn replay(&self) {
        let initialization =
            GameState::initialize_opening(self.game, self.context.clone(), self.setup.clone());
        assert_eq!(
            initialization, self.initialization,
            "full original typed initialization Result replay"
        );
        let mut state = initialization.unwrap().into_state();
        for call in &self.calls {
            let before = state.clone();
            let actual = state.advance(call.input.clone());
            assert_eq!(state, before);
            assert_eq!(
                actual, call.result,
                "full original typed GameInput/Result replay"
            );
            if let Ok(candidate) = actual {
                state = candidate.into_state();
            }
        }
        assert_eq!(state, self.state);
        for deck in &self.decks {
            assert_eq!(
                self.setup
                    .seed
                    .cards
                    .iter()
                    .filter(|card| card.owner == deck.owner)
                    .count(),
                60
            );
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .filter(|card| card.card_id().is_some() && card.owner() == deck.owner)
                    .count(),
                60
            );
            assert_eq!(
                state.execution().window().cards().library(deck.owner).len(),
                52
            );
            assert_eq!(state.execution().life_total(deck.owner), Some(&20.into()));
        }
        assert!(state.outcome().is_none());
        println!(
            "{}",
            json!({"kind":"native-prefix-typed-replay-result","starter":self.decks[0].label,"recorded_full_typed_calls":self.calls.len(),"initialization_full_native_eq":true,"every_original_input_result_full_native_eq":true,"final_full_native_state_eq":true,"completed_game":false,"fully_qualified_cards":0})
        );
    }
}
use spellwake_engine::stack_window::WindowPhase;

#[test]
#[ignore = "requires exact full catalogue/metadata/vocabulary and retained natural60-card deck input; no fallback"]
fn actual_bg_opening_lands_and_intrinsic_mana_fund_source_grizzly_bears_prefix() {
    let mut prefix = Prefix::new("BG");
    let cast = prefix.fund_and_cast(ManaType::Green, BasicLandType::Forest);
    let entry = cast.mana_payment().unwrap().source;
    let resolved = prefix.round(A);
    let permanent = resolved.permanent_entry().unwrap();
    assert_eq!(permanent.entry, entry);
    assert_eq!(permanent.after.card_id(), Some(CardId(1003)));
    assert_eq!(permanent.after.zone(), ObjectZone::Battlefield);
    assert!(!permanent.after.is_token());
    let expected = &prefix
        .setup
        .seed
        .cards
        .iter()
        .find(|card| card.card == CardId(1003))
        .unwrap()
        .program;
    assert_eq!(
        prefix.state.execution().base_program(permanent.after.id()),
        Some(expected)
    );
    let stack_object = cast
        .state()
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1003))
        .unwrap();
    assert_eq!(permanent.before.id(), stack_object);
    assert!(resolved.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if event.cause==EventCause::ResolutionCompleted{entry,completion:EntryCompletion::Resolved} && event.occurrences.iter().any(|row|matches!(&row.value,RulesOccurrence::ZoneChange(change) if change.reason==ZoneReason::PermanentResolution && change.before.object==permanent.before && change.after.object==permanent.after)))));
    prefix.replay();
}

#[test]
#[ignore = "requires exact full catalogue/metadata/vocabulary and retained natural60-card deck input; no fallback"]
fn actual_bw_opening_lands_and_intrinsic_mana_fund_source_raise_with_issued_token_order() {
    let mut prefix = Prefix::new("BW");
    let cast = prefix.fund_and_cast(ManaType::White, BasicLandType::Plains);
    let entry = cast.mana_payment().unwrap().source;
    prefix.round(A);
    let pending = prefix.state.pending_decision().unwrap();
    let RuleFrame::TimestampOrder(frame) = pending.frame() else {
        panic!("actual Raise token-order frame")
    };
    assert_eq!(frame.entry(), entry);
    assert_eq!(frame.instruction(), 0);
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.members().len(), 2);
    let source = &prefix
        .setup
        .seed
        .cards
        .iter()
        .find(|card| card.card == CardId(1003))
        .unwrap()
        .program;
    let ProgramKind::Spell(spell) = &source.kind else {
        panic!("actual Raise source spell")
    };
    let [Instruction::CreateTokens { creations }] = spell.body.instructions.as_slice() else {
        panic!("actual complete Raise source body")
    };
    let [creation] = creations.as_slice() else {
        panic!("one source token group")
    };
    let template = creation
        .definition
        .materialize(prefix.state.execution().creature_vocabulary())
        .unwrap();
    for member in frame.members() {
        assert_eq!(member.creator, A);
        assert_eq!(member.definition, template);
    }
    assert!(
        prefix
            .state
            .execution()
            .window()
            .cards()
            .iter()
            .all(|object| !object.is_token())
    );
    let order = frame
        .members()
        .iter()
        .rev()
        .map(|member| member.member)
        .collect();
    let decision = pending.id();
    let committed = prefix.record(
        A,
        GameCommand::Reply {
            decision,
            choice: DecisionReply::TimestampOrder(order),
        },
    );
    let events: Vec<_> = committed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event)
                if event.cause
                    == EventCause::Instruction {
                        entry,
                        instruction: 0,
                    } =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect();
    let [event] = events.as_slice() else {
        panic!("one actual token creation event")
    };
    let births: Vec<_> = event
        .occurrences
        .iter()
        .map(|row| match &row.value {
            RulesOccurrence::TokenCreated { creator, after } => {
                assert_eq!(*creator, A);
                after.as_ref()
            }
            _ => panic!("token batch contains exact births"),
        })
        .collect();
    assert_eq!(births.len(), 2);
    assert_ne!(births[0].object.id(), births[1].object.id());
    for after in &births {
        assert_eq!(after.object.card_id(), None);
        assert_eq!(after.object.owner(), A);
        assert_eq!(after.object.controller(), Some(A));
        assert_eq!(after.object.zone(), ObjectZone::Battlefield);
        let ObjectOrigin::Token { definition } = after.object.origin() else {
            panic!("real token origin")
        };
        assert_eq!(definition.as_ref(), &template);
        assert_eq!(
            prefix
                .state
                .execution()
                .window()
                .cards()
                .get(after.object.id()),
            Some(&after.object)
        );
    }
    let timestamps: BTreeMap<_, _> = prefix
        .state
        .execution()
        .characteristic_timestamps()
        .collect();
    assert!(
        timestamps[&births[1].object.id()] < timestamps[&births[0].object.id()],
        "actual reverse member choice determines entry timestamps"
    );
    assert!(prefix.state.pending_decision().is_none());
    assert_eq!(
        prefix
            .state
            .execution()
            .window()
            .cards()
            .get(prefix.object(CardId(1003)))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    prefix.replay();
}
