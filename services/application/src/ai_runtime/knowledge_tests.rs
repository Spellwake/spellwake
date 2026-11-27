//! Prompt, real-game, private-look, hidden-information, and invalidation tests
//! for durable seat-scoped revealed-card knowledge (`SeatKnowledgeLedger`).
use super::{
    affordances::PriorityAffordances,
    compact::{compact_prompt, compact_prompt_with_knowledge},
    knowledge::{
        HistoricalRevealView, KnowledgeSource, KnownOpponentHandCard, RevealStatusView,
        SeatKnowledgeLedger, SeatKnowledgeView, VisibleLibraryCard, VisibleLibraryPosition,
    },
};
use crate::{
    native_game::{
        NativeAbilityReference, NativeBinding, NativeDisclosure, NativeDisplayScope,
        NativeDisplaySource, NativeObjectDisplay, NativeProjectionError, NativeProjectionMetadata,
        NativeReference, NativeScope, NativeSeatBinding,
    },
    player_wire::{
        ManaColor, ManaSpend, PlayerAction, PlayerChoice, PlayerCommand, PlayerDomain,
        PlayerFaceDownKind, PlayerPayment, PlayerSnapshot, RulesCount,
    },
};
use serde_json::Value;
use spellwake_contracts::{
    creature_types::CreatureTypeVocabulary,
    names::{EnglishCardName, NameRules, ObjectNames},
};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId},
    characteristics::{
        CastPermissionRule, KeywordAbility, ObjectCharacteristics, StaticAbilityId,
        StaticContinuousAbility,
    },
    combat::{AttackDeclaration, CombatRules},
    effects::{DamageAllocationDomain, LibraryTopReply, Program, SeedCard},
    face_down::FaceUpAbility,
    game::{
        DecisionReply, GameCommand, GameContext, GameId, GameInput, GameState, NativeCandidate,
        OpeningSetup,
    },
    mana::{
        FloatingMana, ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput, ManaType,
    },
    object_identity::{CardId, GameObject, ObjectDestination, ObjectId, ObjectZone},
    occurrence::{ExecutionBindings, WaitingInstance},
    opening::MulliganChoice,
    priority::{PriorityPhase, SeatId},
    program::{
        AffectedPlayers, BodyTarget, CardType, ExecutableBody, ExileAction, Instruction,
        LibrarySearch, ResolvingPlayer, SearchCardMatcher, SearchCount, SearchDestination,
        SearchSource, TriggeredAbility, TriggeredAbilityId, ValueExpr,
    },
    setup::GameSeed,
    shuffle::EntropyTape,
    stack_window::{StackEntry, StackKind, WindowPhase},
    targets::TargetRef,
    token::TokenEntryPreview,
    turn::{Step, TurnStart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU8,
    sync::Arc,
};
use uuid::Uuid;

const SEAT_A: SeatId = SeatId(0);
const SEAT_B: SeatId = SeatId(1);

fn uuid_a() -> Uuid {
    Uuid::from_u128(0xA001)
}

fn uuid_b() -> Uuid {
    Uuid::from_u128(0xB002)
}

fn ctx_id() -> Uuid {
    Uuid::from_u128(0xC003)
}

fn roster() -> Vec<NativeSeatBinding> {
    vec![
        NativeSeatBinding {
            seat_id: uuid_a(),
            native_seat: SEAT_A,
        },
        NativeSeatBinding {
            seat_id: uuid_b(),
            native_seat: SEAT_B,
        },
    ]
}

fn test_vocab() -> Arc<CreatureTypeVocabulary> {
    Arc::new(
        CreatureTypeVocabulary::new(
            "test-knowledge-rev".to_owned(),
            [0x11_u8; 32],
            vec!["Bear".to_owned(), "Goblin".to_owned(), "Ninja".to_owned()],
        )
        .expect("valid test vocabulary"),
    )
}

fn test_context(vocab: &CreatureTypeVocabulary) -> GameContext {
    GameContext::new(
        "knowledge-test-engine".into(),
        vocab.rules_revision().to_owned(),
        "knowledge-test-catalogue".into(),
        "knowledge-test-release".into(),
        "knowledge-test-profile".into(),
        vocab.id(),
        NameRules::literal_names().id(),
    )
    .expect("valid test context")
}

fn named(mut program: Program, name: &str) -> Program {
    program.names = ObjectNames::single(EnglishCardName::new(name.to_owned()).expect("valid name"));
    program
}

struct NamedDisplay {
    context: GameContext,
    cards: BTreeMap<String, crate::player_wire::PlayerCharacteristics>,
}

impl NamedDisplay {
    fn new(context: GameContext) -> Self {
        Self {
            context,
            cards: BTreeMap::new(),
        }
    }
}

impl NativeDisplaySource for NamedDisplay {
    fn context(&self) -> &GameContext {
        &self.context
    }

    fn scope(&self) -> NativeDisplayScope {
        NativeDisplayScope::SourceOnly
    }

    fn object(
        &self,
        _: &GameObject,
        definition: &Program,
    ) -> Result<NativeObjectDisplay, NativeProjectionError> {
        let name = definition
            .names
            .iter()
            .next()
            .map(|n| n.as_str().to_owned())
            .unwrap_or_else(|| "Unnamed Test Card".to_owned());
        let characteristics = self.cards.get(&name).cloned().unwrap_or_else(|| {
            crate::player_wire::PlayerCharacteristics {
                name,
                type_line: "Card".to_owned(),
                rules_text: String::new(),
                mana_cost: None,
                printed_power: None,
                printed_toughness: None,
                printed_loyalty: None,
                printed_defense: None,
            }
        });
        Ok(NativeObjectDisplay {
            definition_id: None,
            printing_id: None,
            custom_card_revision_id: None,
            characteristics,
        })
    }

    fn ability(
        &self,
        _: &GameObject,
        _: &ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError> {
        Ok(match ability {
            NativeAbilityReference::Mana(_) => "Add mana".into(),
            NativeAbilityReference::Activated(_) => "Activated ability".into(),
            NativeAbilityReference::Triggered(_) => "Triggered ability".into(),
            NativeAbilityReference::Continuous(_) => "Static ability".into(),
        })
    }

    fn stack(
        &self,
        entry: &StackEntry,
        _: Option<&ExecutableBody>,
        _: Option<&ExecutionBindings>,
    ) -> Result<String, NativeProjectionError> {
        Ok(match entry.kind() {
            StackKind::CardSpell(_) => "Card spell".into(),
            _ => "Stack entry".into(),
        })
    }

    fn token_preview(&self, _: &TokenEntryPreview) -> Result<String, NativeProjectionError> {
        Ok("Token".into())
    }

    fn trigger(&self, _: &WaitingInstance) -> Result<String, NativeProjectionError> {
        Ok("Trigger".into())
    }

    fn prevention_attempt(
        &self,
        _: &DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError> {
        Ok("Damage".into())
    }
}

struct Harness {
    state: GameState,
    ledger: SeatKnowledgeLedger,
    display: NamedDisplay,
    seq: u64,
}

impl Harness {
    fn scheduled(cards: Vec<SeedCard>, mana: Vec<FloatingMana>, step: Step) -> Self {
        let vocab = test_vocab();
        let context = test_context(&vocab);
        let order = cards
            .iter()
            .filter(|c| matches!(c.destination, ObjectDestination::Battlefield { .. }))
            .map(|c| c.card)
            .collect();
        let seed = GameSeed::with_uniform_life(
            vec![SEAT_A, SEAT_B],
            SEAT_A,
            cards,
            order,
            mana,
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            Arc::new(NameRules::literal_names()),
        );
        let state = GameState::initialize_scheduled(
            GameId(777),
            context.clone(),
            seed,
            TurnStart { number: 1, step },
        )
        .expect("initialize scheduled game")
        .into_state();
        let ledger = SeatKnowledgeLedger::initial(&state, &roster());
        Self {
            state,
            ledger,
            display: NamedDisplay::new(context),
            seq: 0,
        }
    }

    fn opening(cards: Vec<SeedCard>) -> Self {
        let vocab = test_vocab();
        let context = test_context(&vocab);
        let seed = GameSeed::with_uniform_life(
            vec![SEAT_A, SEAT_B],
            SEAT_A,
            cards,
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            vocab,
            Arc::new(NameRules::literal_names()),
        );
        let setup = OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0_u64; 512]),
        };
        let state = GameState::initialize_opening(GameId(778), context.clone(), setup)
            .expect("initialize opening game")
            .into_state();
        let ledger = SeatKnowledgeLedger::initial(&state, &roster());
        Self {
            state,
            ledger,
            display: NamedDisplay::new(context),
            seq: 0,
        }
    }

    /// Initialize an opening game with entropy, have both seats keep, and advance to Seat A's PrecombatMain.
    ///
    /// With all-zero entropy, Fisher-Yates rotates each library left by 1 before drawing 7, so
    /// cards placed at indices `1..=7` in `seat_a_cards` are drawn into Seat A's opening hand and
    /// `seat_a_cards[8..]` followed by `seat_a_cards[0]` remain in Seat A's library from top to bottom.
    fn opening_to_main(seat_a_cards: Vec<(u64, Program)>, seat_b_count: usize) -> Self {
        let mut cards = Vec::new();
        for (id, prog) in seat_a_cards {
            cards.push(seed_card(
                id,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                prog,
            ));
        }
        for i in 0..seat_b_count {
            cards.push(seed_card(
                1000 + i as u64,
                SEAT_B,
                ObjectDestination::LibraryBottom,
                plain_land(&format!("Seat B Land {i}")),
            ));
        }
        let mut h = Self::opening(cards);
        // Both seats keep.
        for _ in 0..2 {
            let d = h.state.pending_decision().expect("opening keep decision");
            let chooser = d.chooser();
            h.advance_native(
                chooser,
                GameCommand::Reply {
                    decision: d.id(),
                    choice: DecisionReply::Mulligan(MulliganChoice::Keep),
                },
            );
        }
        // Advance to Seat A's PrecombatMain.
        for _ in 0..16 {
            if let Some(turn) = h.state.turn()
                && h.state.execution().window().active_player() == SEAT_A
                && turn.position().step == Step::PrecombatMain
            {
                break;
            }
            let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
                h.state.execution().window().phase()
            else {
                panic!("expected priority");
            };
            h.pass(holder);
        }
        h
    }

    fn object_of(&self, card_id: u64) -> ObjectId {
        self.state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(card_id))
            .expect("card exists")
    }

    fn name_of(&self, id: ObjectId) -> String {
        self.state
            .execution()
            .base_program(id)
            .expect("base program")
            .names
            .iter()
            .next()
            .expect("named card")
            .as_str()
            .to_owned()
    }

    fn apply_candidate(&mut self, candidate: NativeCandidate) {
        self.ledger.advance(&self.state, &candidate);
        self.state = candidate.into_state();
        self.seq += 1;
    }

    fn advance_native(&mut self, actor: SeatId, command: GameCommand) {
        let candidate = self
            .state
            .advance(GameInput {
                basis: self.state.basis().clone(),
                actor,
                command,
            })
            .expect("native advance succeeds");
        self.apply_candidate(candidate);
    }

    fn pass(&mut self, actor: SeatId) {
        self.advance_native(actor, GameCommand::Pass);
    }

    fn pass_both(&mut self) {
        let first = match self.state.execution().window().phase() {
            WindowPhase::Priority(PriorityPhase::Ready { holder }) => holder,
            _ => panic!("expected priority holder"),
        };
        let second = if first == SEAT_A { SEAT_B } else { SEAT_A };
        self.pass(first);
        self.pass(second);
    }

    fn scope(&self, viewer: Uuid) -> NativeScope<'_> {
        let binding = NativeBinding::new(ctx_id(), self.state.basis().clone(), roster(), viewer)
            .expect("valid binding");
        NativeScope::new(&self.state, binding).expect("valid scope")
    }

    fn project_and_knowledge(&self, viewer: Uuid) -> (PlayerSnapshot, SeatKnowledgeView, Value) {
        let mut scope = self.scope(viewer);
        let meta = NativeProjectionMetadata {
            through_event_sequence: self.seq.into(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let snap = scope
            .project(&meta, &self.display)
            .expect("projection succeeds");
        let view =
            self.ledger
                .view_for_seat(viewer, &self.state, &self.display, Some(&scope), &snap);
        let (prompt, _) = compact_prompt_with_knowledge(
            &snap,
            &PriorityAffordances::default(),
            Some(&view),
            &[],
            None,
        )
        .expect("compact prompt succeeds");
        assert!(
            !prompt.contains("n1."),
            "compact prompt must never leak raw native handles: {prompt}"
        );
        assert!(
            !prompt.contains(&uuid_a().to_string()) && !prompt.contains(&uuid_b().to_string()),
            "compact prompt must never leak raw seat UUIDs: {prompt}"
        );
        let state_json = prompt
            .split_once("\n\nSTATE:\n")
            .expect("prompt has STATE section")
            .1;
        let state_val: Value = serde_json::from_str(state_json).expect("valid state json");
        (snap, view, state_val)
    }

    fn bridged_action(
        &mut self,
        viewer: Uuid,
        f: impl FnOnce(&mut NativeScope<'_>) -> PlayerAction,
    ) {
        let mut scope = self.scope(viewer);
        let action = f(&mut scope);
        let cmd = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: scope.basis().clone(),
            action,
        };
        let prepared = scope.propose(&cmd).expect("bridged action accepted");
        let candidate = prepared.into_parts().3;
        drop(scope);
        self.apply_candidate(candidate);
    }
}

fn seed_card(id: u64, owner: SeatId, destination: ObjectDestination, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program,
    }
}

fn spend_colorless(n: i64) -> Option<PlayerPayment> {
    Some(PlayerPayment {
        from_lots: Default::default(),
        exile_cast_type: None,
        alternative_cost_source: None,
        graveyard_permission: None,
        use_granted_flashback: false,
        restricted_mana: Vec::new(),
        spend: if n == 0 {
            vec![]
        } else {
            vec![ManaSpend {
                color: ManaColor::Colourless,
                amount: RulesCount::try_from(n.to_string()).unwrap(),
            }]
        },
    })
}

fn plain_creature(name: &str, p: i64, t: i64) -> Program {
    named(
        Program::creature(Some(ManaCost::free()), p.into(), t.into(), BTreeSet::new()),
        name,
    )
}

fn plain_land(name: &str) -> Program {
    named(
        Program::land(
            Default::default(),
            vec![ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::Fixed([(ManaType::Green, 1.into())].into()),
            }],
        ),
        name,
    )
}

fn free_instant(name: &str, target: BodyTarget, instructions: Vec<Instruction>) -> Program {
    named(
        Program::instant(ManaCost::free(), ExecutableBody::new(target, instructions)),
        name,
    )
}

// =========================================================================
// 1. Prompt & Snapshot-Fallback Cases
// =========================================================================

#[test]
fn compact_prompt_with_knowledge_emits_structured_fields_and_deduplicates_cards() {
    let h = Harness::scheduled(
        vec![
            seed_card(
                1,
                SEAT_A,
                ObjectDestination::Hand,
                plain_creature("Alpha Bear", 2, 2),
            ),
            seed_card(
                2,
                SEAT_B,
                ObjectDestination::Hand,
                plain_creature("Hidden Card", 1, 1),
            ),
        ],
        vec![],
        Step::PrecombatMain,
    );
    let (snap, empty_view, empty_state) = h.project_and_knowledge(uuid_a());
    assert!(empty_view.is_empty());
    assert!(empty_state.get("known_opp_hand").is_none());
    assert!(empty_state.get("visible_library").is_none());
    assert!(empty_state.get("historical_reveals").is_none());

    let synthetic_view = SeatKnowledgeView {
        known_opponent_hand: vec![KnownOpponentHandCard {
            owner_seat_id: uuid_b(),
            handle: None,
            characteristics: crate::player_wire::PlayerCharacteristics {
                name: "Counterspell".into(),
                type_line: "Instant".into(),
                rules_text: "Counter target spell.".into(),
                mana_cost: Some("{U}{U}".into()),
                printed_power: None,
                printed_toughness: None,
                printed_loyalty: None,
                printed_defense: None,
            },
            source: KnowledgeSource::HandReveal,
            revealed_turn: Some(2),
            revealed_revision: 10,
        }],
        visible_library_cards: vec![
            VisibleLibraryCard {
                owner_seat_id: uuid_a(),
                handle: None,
                characteristics: crate::player_wire::PlayerCharacteristics {
                    name: "Lightning Bolt".into(),
                    type_line: "Instant".into(),
                    rules_text: "Lightning Bolt deals 3 damage to any target.".into(),
                    mana_cost: Some("{R}".into()),
                    printed_power: None,
                    printed_toughness: None,
                    printed_loyalty: None,
                    printed_defense: None,
                },
                position: VisibleLibraryPosition::Top,
                may_play: true,
                source: KnowledgeSource::PrivateScry,
                revealed_turn: Some(3),
                revealed_revision: 12,
            },
            VisibleLibraryCard {
                owner_seat_id: uuid_b(),
                handle: None,
                characteristics: crate::player_wire::PlayerCharacteristics {
                    name: "Forest".into(),
                    type_line: "Basic Land - Forest".into(),
                    rules_text: String::new(),
                    mana_cost: None,
                    printed_power: None,
                    printed_toughness: None,
                    printed_loyalty: None,
                    printed_defense: None,
                },
                position: VisibleLibraryPosition::BottomUnordered,
                may_play: false,
                source: KnowledgeSource::CascadeReveal,
                revealed_turn: Some(3),
                revealed_revision: 13,
            },
        ],
        historical_reveals: vec![
            HistoricalRevealView {
                owner_seat_id: uuid_b(),
                card_name: "Counterspell".into(),
                source: KnowledgeSource::HandReveal,
                status: RevealStatusView::InOpponentHand,
                revealed_turn: Some(2),
                revealed_revision: 10,
                invalidated_turn: None,
                invalidated_revision: None,
            },
            HistoricalRevealView {
                owner_seat_id: uuid_b(),
                card_name: "Shock".into(),
                source: KnowledgeSource::HandReveal,
                status: RevealStatusView::LeftZone {
                    destination: "graveyard".into(),
                },
                revealed_turn: Some(2),
                revealed_revision: 10,
                invalidated_turn: Some(3),
                invalidated_revision: Some(14),
            },
            HistoricalRevealView {
                owner_seat_id: uuid_a(),
                card_name: "Island".into(),
                source: KnowledgeSource::PrivateScry,
                status: RevealStatusView::InvalidatedByShuffle,
                revealed_turn: Some(1),
                revealed_revision: 5,
                invalidated_turn: Some(2),
                invalidated_revision: Some(8),
            },
            HistoricalRevealView {
                owner_seat_id: uuid_b(),
                card_name: "Opt".into(),
                source: KnowledgeSource::PublicLibraryTop,
                status: RevealStatusView::InvalidatedByLibraryReorder,
                revealed_turn: Some(2),
                revealed_revision: 9,
                invalidated_turn: Some(3),
                invalidated_revision: Some(11),
            },
        ],
    };

    let (prompt, _) = compact_prompt_with_knowledge(
        &snap,
        &PriorityAffordances::default(),
        Some(&synthetic_view),
        &[],
        None,
    )
    .expect("compact prompt");
    assert!(!prompt.contains("n1."));
    assert!(!prompt.contains(&uuid_a().to_string()));
    assert!(!prompt.contains(&uuid_b().to_string()));

    let state: Value = serde_json::from_str(prompt.split_once("\n\nSTATE:\n").unwrap().1).unwrap();
    assert_eq!(state["known_opp_hand"][0]["seat"], "opp");
    assert_eq!(state["known_opp_hand"][0]["card"], "Counterspell");
    assert_eq!(state["known_opp_hand"][0]["source"], "hand_reveal");

    assert_eq!(state["visible_library"][0]["owner"], "you");
    assert_eq!(state["visible_library"][0]["card"], "Lightning Bolt");
    assert_eq!(state["visible_library"][0]["position"], "top");
    assert_eq!(state["visible_library"][0]["may_play"], true);
    assert_eq!(state["visible_library"][1]["owner"], "opp");
    assert_eq!(state["visible_library"][1]["position"], "bottom_unordered");

    assert_eq!(state["historical_reveals"][0]["status"], "in_opponent_hand");
    assert_eq!(
        state["historical_reveals"][1]["status"],
        "left_to_graveyard"
    );
    assert_eq!(
        state["historical_reveals"][2]["status"],
        "invalidated_by_shuffle"
    );
    assert_eq!(
        state["historical_reveals"][3]["status"],
        "invalidated_by_library_reorder"
    );

    assert_eq!(state["cards"]["Counterspell"]["cost"], "{U}{U}");
    assert_eq!(state["cards"]["Lightning Bolt"]["cost"], "{R}");
    assert!(state["cards"]["Forest"].is_object());
    assert!(state["cards"].get("Hidden Card").is_none());
}

#[test]
fn compact_prompt_snapshot_only_fallback_exposes_ninjutsu_and_library_top_handles() {
    let mut ninja = plain_creature("Mistblade Shinobi", 1, 1);
    ninja.activated.push(ActivatedAbility::ninjutsu(
        ActivatedAbilityId(1),
        ManaCost::free(),
    ));
    let attacker =
        plain_creature("Ornithopter", 0, 2).with_printed_keywords([KeywordAbility::Haste]);
    let mut courser = plain_creature("Courser Fixture", 2, 4);
    courser
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(1),
            permission: CastPermissionRule::SeeLibraryTop { public: true },
        });
    let mut h = Harness::scheduled(
        vec![
            seed_card(1, SEAT_A, ObjectDestination::Hand, ninja),
            seed_card(
                2,
                SEAT_A,
                ObjectDestination::Hand,
                plain_creature("Secret Hand Card", 3, 3),
            ),
            seed_card(
                3,
                SEAT_A,
                ObjectDestination::Battlefield { controller: SEAT_A },
                attacker,
            ),
            seed_card(
                4,
                SEAT_A,
                ObjectDestination::Battlefield { controller: SEAT_A },
                courser,
            ),
            seed_card(
                5,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_land("Top Forest"),
            ),
        ],
        vec![],
        Step::DeclareAttackers,
    );
    let d = h.state.pending_decision().unwrap();
    let atk = h.object_of(3);
    h.advance_native(
        d.chooser(),
        GameCommand::Reply {
            decision: d.id(),
            choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: atk,
                defender: SEAT_B,
                defending_planeswalker: None,
            }]),
        },
    );
    h.pass_both();
    let ninja_obj = h.object_of(1);
    h.bridged_action(uuid_a(), |scope| PlayerAction::Ninjutsu {
        source: scope.issue(NativeReference::Object(ninja_obj)).unwrap(),
        ability: scope
            .issue(NativeReference::ActivatedAbility {
                source: ninja_obj,
                ability: ActivatedAbilityId(1),
            })
            .unwrap(),
        attacker: scope.issue(NativeReference::Object(atk)).unwrap(),
        payment: None,
    });

    let mut scope_b = h.scope(uuid_b());
    let snap_b = scope_b
        .project(
            &NativeProjectionMetadata {
                through_event_sequence: h.seq.into(),
                deadline_at: None,
                disclosure: NativeDisclosure::LiveParticipant,
            },
            &h.display,
        )
        .unwrap();
    let (prompt, _) = compact_prompt(&snap_b, &[], &[], None).unwrap();
    assert!(!prompt.contains("Secret Hand Card"));
    assert!(!prompt.contains("n1."));
    let state: Value = serde_json::from_str(prompt.split_once("\n\nSTATE:\n").unwrap().1).unwrap();
    assert_eq!(state["known_opp_hand"].as_array().unwrap().len(), 1);
    assert_eq!(state["known_opp_hand"][0]["id"], "oh1");
    assert_eq!(state["known_opp_hand"][0]["card"], "Mistblade Shinobi");
    assert_eq!(state["visible_library"].as_array().unwrap().len(), 1);
    assert_eq!(state["visible_library"][0]["id"], "elib1");
    assert_eq!(state["visible_library"][0]["card"], "Top Forest");
    assert_eq!(state["visible_library"][0]["position"], "top");
}

// =========================================================================
// 2. Real Native Engine Game Cases
// =========================================================================

#[test]
fn bounce_to_opponent_hand_tracks_bounced_card_and_invalidates_when_cast() {
    let unsummon = free_instant(
        "Unsummon",
        BodyTarget::SingleCreature,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ZoneDest::Hand,
            ),
        ))],
    );
    let mut h = Harness::scheduled(
        vec![
            seed_card(1, SEAT_A, ObjectDestination::Hand, unsummon),
            seed_card(
                2,
                SEAT_B,
                ObjectDestination::Battlefield { controller: SEAT_B },
                plain_creature("Bounced Bear", 2, 2),
            ),
            seed_card(
                3,
                SEAT_B,
                ObjectDestination::Hand,
                plain_creature("Unseen Secret", 4, 4),
            ),
            seed_card(
                4,
                SEAT_B,
                ObjectDestination::LibraryBottom,
                plain_land("Seat B Draw Land"),
            ),
        ],
        vec![],
        Step::PrecombatMain,
    );
    let target_bear = h.object_of(2);
    let spell = h.object_of(1);
    h.advance_native(
        SEAT_A,
        GameCommand::Cast {
            spell,
            target: TargetRef::Object(target_bear),
        },
    );
    h.pass_both();

    let (_, view_a, state_a) = h.project_and_knowledge(uuid_a());
    assert_eq!(view_a.known_opponent_hand.len(), 1);
    assert_eq!(
        view_a.known_opponent_hand[0].characteristics.name,
        "Bounced Bear"
    );
    assert_eq!(
        view_a.known_opponent_hand[0].source,
        KnowledgeSource::PublicZoneToHand
    );
    assert_eq!(state_a["known_opp_hand"][0]["card"], "Bounced Bear");
    assert!(!state_a.to_string().contains("Unseen Secret"));

    let (_, view_b, _) = h.project_and_knowledge(uuid_b());
    assert!(view_b.known_opponent_hand.is_empty());

    // Advance to Seat B's PrecombatMain so Seat B casts Bounced Bear.
    for _ in 0..30 {
        if let Some(turn) = h.state.turn()
            && h.state.execution().window().active_player() == SEAT_B
            && turn.position().step == Step::PrecombatMain
            && h.state.pending_decision().is_none()
        {
            break;
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            h.state.execution().window().phase()
        else {
            panic!("expected priority");
        };
        h.pass(holder);
    }

    let bounced_in_hand = h.object_of(2);
    h.advance_native(
        SEAT_B,
        GameCommand::CastUntargeted {
            spell: bounced_in_hand,
            payment: None,
        },
    );

    let (_, view_a_after, state_a_after) = h.project_and_knowledge(uuid_a());
    assert!(view_a_after.known_opponent_hand.is_empty());
    assert!(state_a_after.get("known_opp_hand").is_none());
    assert!(view_a_after.historical_reveals.iter().any(|r| {
        r.card_name == "Bounced Bear"
            && matches!(r.status, RevealStatusView::LeftZone { ref destination } if destination == "stack")
    }));
}

#[test]
fn ninjutsu_tracks_ninja_and_returned_attacker_then_invalidates_only_ninja_on_resolution() {
    let mut ninja = plain_creature("Ninja of Deep Hours", 2, 2);
    ninja.activated.push(ActivatedAbility::ninjutsu(
        ActivatedAbilityId(1),
        ManaCost::free(),
    ));
    let attacker =
        plain_creature("Returned Scout", 1, 1).with_printed_keywords([KeywordAbility::Haste]);
    let mut h = Harness::scheduled(
        vec![
            seed_card(1, SEAT_A, ObjectDestination::Hand, ninja),
            seed_card(
                2,
                SEAT_A,
                ObjectDestination::Hand,
                plain_creature("Concealed Hand Card", 5, 5),
            ),
            seed_card(
                3,
                SEAT_A,
                ObjectDestination::Battlefield { controller: SEAT_A },
                attacker,
            ),
        ],
        vec![],
        Step::DeclareAttackers,
    );
    let d = h.state.pending_decision().unwrap();
    let atk = h.object_of(3);
    h.advance_native(
        d.chooser(),
        GameCommand::Reply {
            decision: d.id(),
            choice: DecisionReply::DeclareAttackers(vec![AttackDeclaration {
                attacker: atk,
                defender: SEAT_B,
                defending_planeswalker: None,
            }]),
        },
    );
    h.pass_both();
    let ninja_obj = h.object_of(1);
    h.bridged_action(uuid_a(), |scope| PlayerAction::Ninjutsu {
        source: scope.issue(NativeReference::Object(ninja_obj)).unwrap(),
        ability: scope
            .issue(NativeReference::ActivatedAbility {
                source: ninja_obj,
                ability: ActivatedAbilityId(1),
            })
            .unwrap(),
        attacker: scope.issue(NativeReference::Object(atk)).unwrap(),
        payment: None,
    });

    let (_, view_b_mid, state_b_mid) = h.project_and_knowledge(uuid_b());
    let names_mid: BTreeSet<&str> = view_b_mid
        .known_opponent_hand
        .iter()
        .map(|c| c.characteristics.name.as_str())
        .collect();
    assert_eq!(
        names_mid,
        BTreeSet::from(["Ninja of Deep Hours", "Returned Scout"])
    );
    assert!(!state_b_mid.to_string().contains("Concealed Hand Card"));
    assert!(view_b_mid.known_opponent_hand.iter().any(|c| {
        c.characteristics.name == "Ninja of Deep Hours"
            && c.source == KnowledgeSource::NinjutsuReveal
            && c.handle.is_some()
    }));

    h.pass_both();
    let (_, view_b_after, state_b_after) = h.project_and_knowledge(uuid_b());
    assert_eq!(view_b_after.known_opponent_hand.len(), 1);
    assert_eq!(
        view_b_after.known_opponent_hand[0].characteristics.name,
        "Returned Scout"
    );
    assert_eq!(
        view_b_after.known_opponent_hand[0].source,
        KnowledgeSource::PublicZoneToHand
    );
    assert!(!state_b_after.to_string().contains("Concealed Hand Card"));
    assert!(view_b_after.historical_reveals.iter().any(|r| {
        r.card_name == "Ninja of Deep Hours"
            && matches!(r.status, RevealStatusView::LeftZone { ref destination } if destination == "battlefield")
    }));
}

#[test]
fn scry_and_surveil_are_private_to_actor_and_invalidated_by_shuffle() {
    let scry2 = free_instant(
        "Scry Two",
        BodyTarget::None,
        vec![Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::scry(
                ResolvingPlayer::Controller,
                ValueExpr::literal(2),
            ),
        ))],
    );
    let surveil2 = free_instant(
        "Surveil Two",
        BodyTarget::None,
        vec![Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::surveil(
                ResolvingPlayer::Controller,
                ValueExpr::literal(2),
            ),
        ))],
    );
    let shuffle = free_instant(
        "Shuffle Self",
        BodyTarget::None,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::shuffle(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
            ),
        ))],
    );
    // Index 0 rotates to bottom of library; indices 1..=7 are drawn into hand;
    // indices 8..=10 stay at top of library in order.
    let mut h = Harness::opening_to_main(
        vec![
            (13, plain_creature("Lib Card Four", 4, 4)),
            (1, scry2),
            (2, surveil2),
            (3, shuffle),
            (4, plain_land("Hand Land 1")),
            (5, plain_land("Hand Land 2")),
            (6, plain_land("Hand Land 3")),
            (7, plain_land("Hand Land 4")),
            (10, plain_creature("Lib Card One", 1, 1)),
            (11, plain_creature("Lib Card Two", 2, 2)),
            (12, plain_creature("Lib Card Three", 3, 3)),
        ],
        10,
    );

    let lib = h
        .state
        .execution()
        .window()
        .cards()
        .library(SEAT_A)
        .to_vec();
    let top_0 = lib[0];
    let top_1 = lib[1];
    let top_2 = lib[2];
    let name_0 = h.name_of(top_0);
    let name_1 = h.name_of(top_1);
    let name_2 = h.name_of(top_2);

    // 1. Cast Scry 2: keep top_0 on top, put top_1 on bottom.
    let scry_spell = h.object_of(1);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: scry_spell,
            payment: None,
        },
    );
    h.pass_both();
    let d_scry = h.state.pending_decision().expect("scry decision");
    assert_eq!(d_scry.chooser(), SEAT_A);
    let (_, view_a_pending, _) = h.project_and_knowledge(uuid_a());
    let (_, view_b_pending, state_b_pending) = h.project_and_knowledge(uuid_b());
    assert_eq!(view_a_pending.visible_library_cards.len(), 2);
    assert!(view_b_pending.visible_library_cards.is_empty());
    assert!(!state_b_pending.to_string().contains(&name_0));
    assert!(!state_b_pending.to_string().contains(&name_1));

    h.advance_native(
        SEAT_A,
        GameCommand::Reply {
            decision: d_scry.id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first: vec![top_0],
                bottom_top_first: vec![top_1],
            }),
        },
    );

    let (_, view_a_scry, state_a_scry) = h.project_and_knowledge(uuid_a());
    let (_, view_b_scry, state_b_scry) = h.project_and_knowledge(uuid_b());
    assert!(view_a_scry.visible_library_cards.iter().any(|c| {
        c.characteristics.name == name_0
            && c.position == VisibleLibraryPosition::Top
            && c.source == KnowledgeSource::PrivateScry
    }));
    assert!(view_a_scry.visible_library_cards.iter().any(|c| {
        c.characteristics.name == name_1
            && c.position == VisibleLibraryPosition::Bottom
            && c.source == KnowledgeSource::PrivateScry
    }));
    assert_eq!(state_a_scry["visible_library"].as_array().unwrap().len(), 2);
    assert!(view_b_scry.visible_library_cards.is_empty());
    assert!(view_b_scry.historical_reveals.is_empty());
    assert!(!state_b_scry.to_string().contains(&name_0));
    assert!(!state_b_scry.to_string().contains(&name_1));

    // 2. Cast Surveil 2: top 2 are top_0 and top_2. Put top_0 in graveyard, keep top_2 on top.
    let surveil_spell = h.object_of(2);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: surveil_spell,
            payment: None,
        },
    );
    h.pass_both();
    let d_surv = h.state.pending_decision().expect("surveil decision");
    h.advance_native(
        SEAT_A,
        GameCommand::Reply {
            decision: d_surv.id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Surveil {
                top_first: vec![top_2],
                to_graveyard: vec![top_0],
            }),
        },
    );
    let (_, view_a_surv, _) = h.project_and_knowledge(uuid_a());
    let (_, view_b_surv, state_b_surv) = h.project_and_knowledge(uuid_b());
    assert!(view_a_surv.visible_library_cards.iter().any(|c| {
        c.characteristics.name == name_2
            && c.position == VisibleLibraryPosition::Top
            && c.source == KnowledgeSource::PrivateSurveil
    }));
    assert!(view_a_surv.visible_library_cards.iter().any(|c| {
        c.characteristics.name == name_1 && c.position == VisibleLibraryPosition::Bottom
    }));
    assert!(
        view_a_surv
            .visible_library_cards
            .iter()
            .all(|c| c.characteristics.name != name_0)
    );
    assert!(view_b_surv.visible_library_cards.is_empty());
    assert!(!state_b_surv.to_string().contains(&name_2));
    assert!(!state_b_surv.to_string().contains(&name_1));

    // 3. Cast ShuffleOwnLibrary: ALL tracked library positions for Seat A are invalidated!
    let shuffle_spell = h.object_of(3);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: shuffle_spell,
            payment: None,
        },
    );
    h.pass_both();
    let (_, view_a_shuf, state_a_shuf) = h.project_and_knowledge(uuid_a());
    assert!(view_a_shuf.visible_library_cards.is_empty());
    assert!(state_a_shuf.get("visible_library").is_none());
    assert!(
        view_a_shuf.historical_reveals.iter().any(|r| {
            r.card_name == name_2 && r.status == RevealStatusView::InvalidatedByShuffle
        })
    );
    assert!(
        view_a_shuf.historical_reveals.iter().any(|r| {
            r.card_name == name_1 && r.status == RevealStatusView::InvalidatedByShuffle
        })
    );
}

#[test]
fn opponent_scry_invalidates_prior_public_library_top_knowledge_without_leaking_order() {
    let scry2 = free_instant(
        "Scry Two",
        BodyTarget::None,
        vec![Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::scry(
                ResolvingPlayer::Controller,
                ValueExpr::literal(2),
            ),
        ))],
    );
    let destroy = free_instant(
        "Destroy Courser",
        BodyTarget::SinglePermanent,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::Targets,
                spellwake_engine::toolbox::ZoneDest::Graveyard {
                    semantics: spellwake_engine::toolbox::GraveyardSemantics::Destroy {
                        allow_regenerate: true,
                    },
                },
            ),
        ))],
    );
    let mut courser = plain_creature("Public Top Seer", 2, 4);
    courser
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(1),
            permission: CastPermissionRule::SeeLibraryTop { public: true },
        });
    let mut h = Harness::scheduled(
        vec![
            seed_card(
                1,
                SEAT_A,
                ObjectDestination::Battlefield { controller: SEAT_A },
                courser,
            ),
            seed_card(2, SEAT_A, ObjectDestination::Hand, destroy),
            seed_card(3, SEAT_A, ObjectDestination::Hand, scry2),
            seed_card(
                10,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_creature("Known Top", 1, 1),
            ),
            seed_card(
                11,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_creature("Hidden Second", 2, 2),
            ),
            seed_card(
                12,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_creature("Hidden Third", 3, 3),
            ),
        ],
        vec![],
        Step::PrecombatMain,
    );

    let (_, view_b_init, _) = h.project_and_knowledge(uuid_b());
    assert_eq!(view_b_init.visible_library_cards.len(), 1);
    assert_eq!(
        view_b_init.visible_library_cards[0].characteristics.name,
        "Known Top"
    );

    let destroy_obj = h.object_of(2);
    let courser_obj = h.object_of(1);
    h.advance_native(
        SEAT_A,
        GameCommand::Cast {
            spell: destroy_obj,
            target: TargetRef::Object(courser_obj),
        },
    );
    h.pass_both();
    let (_, view_b_rem, _) = h.project_and_knowledge(uuid_b());
    assert_eq!(view_b_rem.visible_library_cards.len(), 1);
    assert_eq!(
        view_b_rem.visible_library_cards[0].characteristics.name,
        "Known Top"
    );

    let scry_obj = h.object_of(3);
    let known_top_obj = h.object_of(10);
    let hidden_second_obj = h.object_of(11);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: scry_obj,
            payment: None,
        },
    );
    h.pass_both();
    let d = h.state.pending_decision().expect("scry decision");
    h.advance_native(
        SEAT_A,
        GameCommand::Reply {
            decision: d.id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first: vec![hidden_second_obj],
                bottom_top_first: vec![known_top_obj],
            }),
        },
    );

    let (_, view_b_after, state_b_after) = h.project_and_knowledge(uuid_b());
    assert!(view_b_after.visible_library_cards.is_empty());
    assert!(view_b_after.historical_reveals.iter().any(|r| {
        r.card_name == "Known Top" && r.status == RevealStatusView::InvalidatedByLibraryReorder
    }));
    assert!(!state_b_after.to_string().contains("Hidden Second"));
}

#[test]
fn public_vs_private_library_top_access_and_draw_transfer_to_opponent_hand() {
    let mut public_seer = plain_creature("Courser Fixture", 2, 4);
    public_seer
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(1),
            permission: CastPermissionRule::SeeLibraryTop { public: true },
        });
    let draw1 = free_instant(
        "Draw One",
        BodyTarget::None,
        vec![Instruction::DrawCards(Box::new(
            spellwake_engine::toolbox::DrawCardsSpec::simple(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
                ValueExpr::literal(1),
            ),
        ))],
    );
    let mut h_pub = Harness::scheduled(
        vec![
            seed_card(
                1,
                SEAT_A,
                ObjectDestination::Battlefield { controller: SEAT_A },
                public_seer,
            ),
            seed_card(2, SEAT_A, ObjectDestination::Hand, draw1.clone()),
            seed_card(
                10,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_creature("Public Top Draw", 3, 3),
            ),
            seed_card(
                11,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_creature("Next Public Top", 4, 4),
            ),
        ],
        vec![],
        Step::PrecombatMain,
    );
    let (_, view_b_before, _) = h_pub.project_and_knowledge(uuid_b());
    assert_eq!(view_b_before.visible_library_cards.len(), 1);
    assert_eq!(
        view_b_before.visible_library_cards[0].characteristics.name,
        "Public Top Draw"
    );

    let draw_obj = h_pub.object_of(2);
    h_pub.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: draw_obj,
            payment: None,
        },
    );
    h_pub.pass_both();

    let (_, view_b_after, state_b_after) = h_pub.project_and_knowledge(uuid_b());
    assert_eq!(view_b_after.known_opponent_hand.len(), 1);
    assert_eq!(
        view_b_after.known_opponent_hand[0].characteristics.name,
        "Public Top Draw"
    );
    assert_eq!(
        view_b_after.known_opponent_hand[0].source,
        KnowledgeSource::DrawnFromKnownLibrary
    );
    assert_eq!(
        state_b_after["known_opp_hand"][0]["card"],
        "Public Top Draw"
    );
    assert_eq!(view_b_after.visible_library_cards.len(), 1);
    assert_eq!(
        view_b_after.visible_library_cards[0].characteristics.name,
        "Next Public Top"
    );

    let mut private_seer = plain_creature("Vizier Fixture", 3, 4);
    private_seer
        .additional_battlefield_abilities
        .push(StaticContinuousAbility::CastPermission {
            id: StaticAbilityId(1),
            permission: CastPermissionRule::SeeLibraryTop { public: false },
        });
    let mut h_priv = Harness::scheduled(
        vec![
            seed_card(
                1,
                SEAT_A,
                ObjectDestination::Battlefield { controller: SEAT_A },
                private_seer,
            ),
            seed_card(2, SEAT_A, ObjectDestination::Hand, draw1),
            seed_card(
                10,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_creature("Private Top Card", 3, 3),
            ),
            seed_card(
                11,
                SEAT_A,
                ObjectDestination::LibraryBottom,
                plain_creature("Next Private Top", 4, 4),
            ),
        ],
        vec![],
        Step::PrecombatMain,
    );
    let (_, view_a_priv, _) = h_priv.project_and_knowledge(uuid_a());
    let (_, view_b_priv, state_b_priv) = h_priv.project_and_knowledge(uuid_b());
    assert_eq!(view_a_priv.visible_library_cards.len(), 1);
    assert_eq!(
        view_a_priv.visible_library_cards[0].characteristics.name,
        "Private Top Card"
    );
    assert_eq!(
        view_a_priv.visible_library_cards[0].source,
        KnowledgeSource::PrivateLibraryTop
    );
    assert!(view_b_priv.visible_library_cards.is_empty());
    assert!(view_b_priv.known_opponent_hand.is_empty());
    assert!(!state_b_priv.to_string().contains("Private Top Card"));

    let draw_obj = h_priv.object_of(2);
    h_priv.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: draw_obj,
            payment: None,
        },
    );
    h_priv.pass_both();

    let (_, view_b_priv_after, state_b_priv_after) = h_priv.project_and_knowledge(uuid_b());
    assert!(view_b_priv_after.known_opponent_hand.is_empty());
    assert!(view_b_priv_after.visible_library_cards.is_empty());
    assert!(view_b_priv_after.historical_reveals.is_empty());
    assert!(!state_b_priv_after.to_string().contains("Private Top Card"));
    assert!(!state_b_priv_after.to_string().contains("Next Private Top"));
}

#[test]
fn public_and_private_library_searches_scope_reveals_and_invalidate_shuffled_cards() {
    let public_tutor = free_instant(
        "Public Tutor",
        BodyTarget::None,
        vec![Instruction::SearchLibrary {
            filter: Box::new(LibrarySearch::card_type_to_hand(CardType::Creature)),
        }],
    );
    let private_hand_tutor = free_instant(
        "Demonic Fixture",
        BodyTarget::None,
        vec![Instruction::SearchLibrary {
            filter: Box::new(LibrarySearch::any_to_hand(NonZeroU8::new(1).unwrap())),
        }],
    );
    let private_top_tutor = free_instant(
        "Vampiric Fixture",
        BodyTarget::None,
        vec![Instruction::SearchLibrary {
            filter: Box::new(LibrarySearch {
                searcher: ResolvingPlayer::Controller,
                source: SearchSource::OwnLibrary,
                matcher: SearchCardMatcher::Any,
                mana_value: None,
                count: SearchCount::UpTo(1),
                pair_constraint: None,
                destination: SearchDestination::LibraryTop,
                reveal: false,
            }),
        }],
    );
    let scry1 = free_instant(
        "Scry One",
        BodyTarget::None,
        vec![Instruction::MutatePlayer(Box::new(
            spellwake_engine::toolbox::MutatePlayerSpec::scry(
                ResolvingPlayer::Controller,
                ValueExpr::literal(1),
            ),
        ))],
    );
    let mut h = Harness::opening_to_main(
        vec![
            (14, plain_land("Unsearched Forest")),
            (1, scry1),
            (2, public_tutor),
            (3, private_hand_tutor),
            (4, private_top_tutor),
            (5, plain_land("Filler 1")),
            (6, plain_land("Filler 2")),
            (7, plain_land("Filler 3")),
            (10, plain_creature("Initial Top Bear", 2, 2)),
            (11, plain_creature("Searched Public Bear", 3, 3)),
            (12, plain_creature("Secret Demonic Target", 4, 4)),
            (13, plain_creature("Secret Vampiric Top", 5, 5)),
        ],
        10,
    );

    let initial_top = h.state.execution().window().cards().library(SEAT_A)[0];
    let initial_top_name = h.name_of(initial_top);

    // First Scry 1 so Seat A knows the top card.
    let scry_obj = h.object_of(1);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: scry_obj,
            payment: None,
        },
    );
    h.pass_both();
    let d_scry = h.state.pending_decision().unwrap();
    h.advance_native(
        SEAT_A,
        GameCommand::Reply {
            decision: d_scry.id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Scry {
                top_first: vec![initial_top],
                bottom_top_first: vec![],
            }),
        },
    );
    assert_eq!(
        h.project_and_knowledge(uuid_a())
            .1
            .visible_library_cards
            .len(),
        1
    );

    // 1. Public search to hand (`reveal = true`): shuffles library (invalidating prior top)
    // and discloses Searched Public Bear to Seat B in `known_opp_hand`.
    let pub_obj = h.object_of(2);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: pub_obj,
            payment: None,
        },
    );
    h.pass_both();
    let d_pub = h.state.pending_decision().expect("public search decision");
    let pub_target = h.object_of(11);
    h.advance_native(
        SEAT_A,
        GameCommand::Reply {
            decision: d_pub.id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(pub_target),
            }),
        },
    );
    let (_, view_a_pub, _) = h.project_and_knowledge(uuid_a());
    let (_, view_b_pub, state_b_pub) = h.project_and_knowledge(uuid_b());
    assert!(view_a_pub.visible_library_cards.is_empty());
    assert!(view_a_pub.historical_reveals.iter().any(|r| {
        r.card_name == initial_top_name && r.status == RevealStatusView::InvalidatedByShuffle
    }));
    assert_eq!(view_b_pub.known_opponent_hand.len(), 1);
    assert_eq!(
        view_b_pub.known_opponent_hand[0].characteristics.name,
        "Searched Public Bear"
    );
    assert_eq!(
        view_b_pub.known_opponent_hand[0].source,
        KnowledgeSource::SearchReveal
    );
    assert!(!state_b_pub.to_string().contains("Unsearched Forest"));

    // 2. Private search to hand (`reveal = false`): Seat B learns NOTHING new!
    let priv_hand_obj = h.object_of(3);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: priv_hand_obj,
            payment: None,
        },
    );
    h.pass_both();
    let d_priv_h = h.state.pending_decision().expect("private hand search");
    let demonic_target = h.object_of(12);
    h.advance_native(
        SEAT_A,
        GameCommand::Reply {
            decision: d_priv_h.id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(demonic_target),
            }),
        },
    );
    let (_, view_b_priv_h, state_b_priv_h) = h.project_and_knowledge(uuid_b());
    assert_eq!(view_b_priv_h.known_opponent_hand.len(), 1);
    assert!(!state_b_priv_h.to_string().contains("Secret Demonic Target"));

    // 3. Private search to top of library (`reveal = false`, `LibraryTop`):
    // ONLY searcher Seat A knows Secret Vampiric Top is on Top (`private_search`), Seat B sees NOTHING!
    let priv_top_obj = h.object_of(4);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: priv_top_obj,
            payment: None,
        },
    );
    h.pass_both();
    let d_priv_t = h.state.pending_decision().expect("private top search");
    let vamp_target = h.object_of(13);
    h.advance_native(
        SEAT_A,
        GameCommand::Reply {
            decision: d_priv_t.id(),
            choice: DecisionReply::LibraryTop(LibraryTopReply::Search {
                selected: Some(vamp_target),
            }),
        },
    );
    let (_, view_a_vamp, state_a_vamp) = h.project_and_knowledge(uuid_a());
    let (_, view_b_vamp, state_b_vamp) = h.project_and_knowledge(uuid_b());
    assert_eq!(view_a_vamp.visible_library_cards.len(), 1);
    assert_eq!(
        view_a_vamp.visible_library_cards[0].characteristics.name,
        "Secret Vampiric Top"
    );
    assert_eq!(
        view_a_vamp.visible_library_cards[0].position,
        VisibleLibraryPosition::Top
    );
    assert_eq!(
        view_a_vamp.visible_library_cards[0].source,
        KnowledgeSource::PrivateSearch
    );
    assert_eq!(
        state_a_vamp["visible_library"][0]["card"],
        "Secret Vampiric Top"
    );
    assert!(view_b_vamp.visible_library_cards.is_empty());
    assert!(!state_b_vamp.to_string().contains("Secret Vampiric Top"));
}

#[test]
fn hand_reveal_tracks_remaining_cards_and_invalidates_discarded_and_played_cards() {
    let peek_and_discard = free_instant(
        "Peek and Discard Fixture",
        BodyTarget::SingleOpponent,
        vec![
            Instruction::ZoneOp(Box::new(spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::TargetPlayer,
                        ),
                    )),
                    spellwake_engine::program::ObjectFilter::any(),
                ),
                spellwake_engine::toolbox::ZoneDest::RevealOnly,
            ))),
            Instruction::Discard {
                players: AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                amount: ValueExpr::literal(1),
            },
        ],
    );
    let mut h = Harness::scheduled(
        vec![
            seed_card(1, SEAT_A, ObjectDestination::Hand, peek_and_discard),
            seed_card(
                2,
                SEAT_B,
                ObjectDestination::Hand,
                free_instant(
                    "Revealed Spell",
                    BodyTarget::None,
                    vec![Instruction::Jump { destination: 1 }],
                ),
            ),
            seed_card(
                5,
                SEAT_B,
                ObjectDestination::LibraryBottom,
                plain_land("Seat B Draw Land"),
            ),
            seed_card(
                3,
                SEAT_B,
                ObjectDestination::Hand,
                plain_land("Revealed Forest"),
            ),
            seed_card(
                4,
                SEAT_B,
                ObjectDestination::Hand,
                plain_creature("Revealed Bear", 2, 2),
            ),
        ],
        vec![],
        Step::PrecombatMain,
    );
    let duress_obj = h.object_of(1);
    h.advance_native(
        SEAT_A,
        GameCommand::Cast {
            spell: duress_obj,
            target: TargetRef::Player(SEAT_B),
        },
    );
    h.pass_both();

    let d = h.state.pending_decision().expect("discard decision");
    assert_eq!(d.chooser(), SEAT_B);
    let (_, view_a_mid, _) = h.project_and_knowledge(uuid_a());
    assert_eq!(view_a_mid.known_opponent_hand.len(), 3);

    let spell_obj = h.object_of(2);
    h.advance_native(
        SEAT_B,
        GameCommand::Reply {
            decision: d.id(),
            choice: DecisionReply::SelectResolution(vec![spell_obj]),
        },
    );

    let (_, view_a_after_discard, _) = h.project_and_knowledge(uuid_a());
    let rem_names: BTreeSet<&str> = view_a_after_discard
        .known_opponent_hand
        .iter()
        .map(|c| c.characteristics.name.as_str())
        .collect();
    assert_eq!(
        rem_names,
        BTreeSet::from(["Revealed Forest", "Revealed Bear"])
    );
    assert!(view_a_after_discard.historical_reveals.iter().any(|r| {
        r.card_name == "Revealed Spell"
            && matches!(r.status, RevealStatusView::LeftZone { ref destination } if destination == "graveyard")
    }));

    for _ in 0..30 {
        if let Some(turn) = h.state.turn()
            && h.state.execution().window().active_player() == SEAT_B
            && turn.position().step == Step::PrecombatMain
        {
            break;
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            h.state.execution().window().phase()
        else {
            panic!("expected priority");
        };
        h.pass(holder);
    }
    let land_obj = h.object_of(3);
    h.advance_native(SEAT_B, GameCommand::PlayLand { land: land_obj });

    let (_, view_a_after_land, _) = h.project_and_knowledge(uuid_a());
    assert_eq!(view_a_after_land.known_opponent_hand.len(), 1);
    assert_eq!(
        view_a_after_land.known_opponent_hand[0]
            .characteristics
            .name,
        "Revealed Bear"
    );
    assert!(view_a_after_land.historical_reveals.iter().any(|r| {
        r.card_name == "Revealed Forest"
            && matches!(r.status, RevealStatusView::LeftZone { ref destination } if destination == "battlefield")
    }));
}

#[test]
fn london_mulligan_bottom_is_private_to_owner_and_invalidated_on_shuffle() {
    let mut cards = Vec::new();
    let shuffle = free_instant(
        "Shuffle Self",
        BodyTarget::None,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::shuffle(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
            ),
        ))],
    );
    for i in 1..=12_u64 {
        let prog = if i <= 6 {
            shuffle.clone()
        } else {
            plain_land(&format!("Seat A Land {i}"))
        };
        cards.push(seed_card(i, SEAT_A, ObjectDestination::LibraryBottom, prog));
    }
    for i in 21..=32_u64 {
        cards.push(seed_card(
            i,
            SEAT_B,
            ObjectDestination::LibraryBottom,
            plain_land(&format!("Seat B Land {i}")),
        ));
    }
    let mut h = Harness::opening(cards);

    let mut seat_a_mulliganed = false;
    let mut bottom_card_name = String::new();
    for _ in 0..8 {
        let Some(d) = h.state.pending_decision() else {
            break;
        };
        let chooser = d.chooser();
        match d.frame() {
            spellwake_engine::game::RuleFrame::Opening(
                spellwake_engine::opening::OpeningDecision::Declare { .. },
            ) => {
                let choice = if chooser == SEAT_A && !seat_a_mulliganed {
                    seat_a_mulliganed = true;
                    MulliganChoice::Mulligan
                } else {
                    MulliganChoice::Keep
                };
                h.advance_native(
                    chooser,
                    GameCommand::Reply {
                        decision: d.id(),
                        choice: DecisionReply::Mulligan(choice),
                    },
                );
            }
            spellwake_engine::game::RuleFrame::Opening(
                spellwake_engine::opening::OpeningDecision::Bottom { .. },
            ) => {
                assert_eq!(chooser, SEAT_A);
                let bottom_card = h
                    .state
                    .execution()
                    .window()
                    .cards()
                    .iter()
                    .find(|o| {
                        o.owner() == SEAT_A
                            && o.zone() == ObjectZone::Hand(SEAT_A)
                            && o.card_id().is_some_and(|c| c.0 >= 7)
                    })
                    .map(GameObject::id)
                    .expect("a land card in Seat A hand");
                bottom_card_name = h.name_of(bottom_card);
                h.advance_native(
                    SEAT_A,
                    GameCommand::Reply {
                        decision: d.id(),
                        choice: DecisionReply::Bottom(vec![bottom_card]),
                    },
                );
            }
            other => panic!("unexpected opening decision: {other:?}"),
        }
    }
    assert!(!bottom_card_name.is_empty());

    let (_, view_a, state_a) = h.project_and_knowledge(uuid_a());
    let (_, view_b, state_b) = h.project_and_knowledge(uuid_b());
    assert_eq!(view_a.visible_library_cards.len(), 1);
    assert_eq!(
        view_a.visible_library_cards[0].characteristics.name,
        bottom_card_name
    );
    assert_eq!(
        view_a.visible_library_cards[0].position,
        VisibleLibraryPosition::Bottom
    );
    assert_eq!(
        view_a.visible_library_cards[0].source,
        KnowledgeSource::PrivateMulliganBottom
    );
    assert_eq!(state_a["visible_library"][0]["position"], "bottom");
    assert!(view_b.visible_library_cards.is_empty());
    assert!(view_b.historical_reveals.is_empty());
    assert!(!state_b.to_string().contains(&bottom_card_name));

    for _ in 0..10 {
        if let Some(turn) = h.state.turn()
            && turn.position().step == Step::PrecombatMain
        {
            break;
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            h.state.execution().window().phase()
        else {
            panic!("expected priority");
        };
        h.pass(holder);
    }
    let shuffle_in_hand = h
        .state
        .execution()
        .window()
        .cards()
        .iter()
        .find(|o| {
            o.owner() == SEAT_A
                && o.zone() == ObjectZone::Hand(SEAT_A)
                && o.card_id().is_some_and(|c| c.0 <= 6)
        })
        .map(GameObject::id)
        .expect("a shuffle spell in Seat A hand");
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: shuffle_in_hand,
            payment: None,
        },
    );
    h.pass_both();

    let (_, view_a_after, _) = h.project_and_knowledge(uuid_a());
    assert!(view_a_after.visible_library_cards.is_empty());
    assert!(view_a_after.historical_reveals.iter().any(|r| {
        r.card_name == bottom_card_name && r.status == RevealStatusView::InvalidatedByShuffle
    }));
}

#[test]
fn morph_and_foretell_hidden_information_never_leaks_and_invalidates_prior_hand_knowledge() {
    let mut morph = named(
        Program::creature(
            Some(ManaCost::generic(4.into())),
            5.into(),
            4.into(),
            BTreeSet::new(),
        ),
        "Secret Morph Titan",
    );
    morph
        .face_up_abilities
        .push(FaceUpAbility::Morph(ManaCost::generic(0.into())));
    let mut foretell = named(
        Program::creature(
            Some(ManaCost::generic(4.into())),
            5.into(),
            4.into(),
            BTreeSet::new(),
        ),
        "Secret Foretell Drake",
    );
    foretell.exile_action = Some(ExileAction::Foretell(ManaCost::generic(1.into())));
    let peek = free_instant(
        "Peek Hand",
        BodyTarget::SingleOpponent,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::move_all(
                spellwake_engine::toolbox::ObjectQuery::zone(
                    spellwake_engine::toolbox::QueryZone::Hand(Box::new(
                        spellwake_engine::toolbox::PlayerSelector::Resolving(
                            ResolvingPlayer::TargetPlayer,
                        ),
                    )),
                    spellwake_engine::program::ObjectFilter::any(),
                ),
                spellwake_engine::toolbox::ZoneDest::RevealOnly,
            ),
        ))],
    );

    let mut h = Harness::scheduled(
        vec![
            seed_card(1, SEAT_A, ObjectDestination::Hand, morph),
            seed_card(2, SEAT_A, ObjectDestination::Hand, foretell.clone()),
            seed_card(
                3,
                SEAT_A,
                ObjectDestination::Hand,
                plain_creature("Decoy Discard", 1, 1),
            ),
            seed_card(4, SEAT_B, ObjectDestination::Hand, peek.clone()),
        ],
        vec![FloatingMana {
            player: SEAT_A,
            mana_type: ManaType::Colourless,
            amount: 5.into(),
        }],
        Step::PrecombatMain,
    );

    let f_obj = h.object_of(2);
    h.bridged_action(uuid_a(), |scope| PlayerAction::Foretell {
        source: scope.issue(NativeReference::Object(f_obj)).unwrap(),
        payment: spend_colorless(2),
    });
    let m_obj = h.object_of(1);
    h.bridged_action(uuid_a(), |scope| PlayerAction::CastFaceDown {
        source: scope.issue(NativeReference::Object(m_obj)).unwrap(),
        appearance: PlayerFaceDownKind::Morph,
        payment: spend_colorless(3),
    });
    h.pass_both();

    let (_, view_b_hidden, state_b_hidden) = h.project_and_knowledge(uuid_b());
    assert!(view_b_hidden.known_opponent_hand.is_empty());
    assert!(view_b_hidden.visible_library_cards.is_empty());
    assert!(view_b_hidden.historical_reveals.is_empty());
    assert!(!state_b_hidden.to_string().contains("Secret Morph Titan"));
    assert!(!state_b_hidden.to_string().contains("Secret Foretell Drake"));

    let m_bf = h.object_of(1);
    h.bridged_action(uuid_a(), |scope| PlayerAction::TurnFaceUp {
        source: scope.issue(NativeReference::Object(m_bf)).unwrap(),
        ability: 0,
        payment: spend_colorless(0),
    });
    let (_, view_b_turnup, state_b_turnup) = h.project_and_knowledge(uuid_b());
    assert!(!state_b_turnup.to_string().contains("Secret Foretell Drake"));
    assert!(
        view_b_turnup
            .historical_reveals
            .iter()
            .all(|r| r.card_name != "Secret Foretell Drake")
    );

    // If Seat B previously saw Foretell Card in Seat A's hand, foretelling it removes it
    // from `known_opp_hand` with `RevealStatusView::Invalidated` (never leaking face-down exile).
    let mut h2 = Harness::scheduled(
        vec![
            seed_card(1, SEAT_A, ObjectDestination::Hand, foretell),
            seed_card(
                2,
                SEAT_A,
                ObjectDestination::Hand,
                plain_creature("Decoy", 1, 1),
            ),
            seed_card(3, SEAT_B, ObjectDestination::Hand, peek),
        ],
        vec![FloatingMana {
            player: SEAT_A,
            mana_type: ManaType::Colourless,
            amount: 2.into(),
        }],
        Step::PrecombatMain,
    );
    h2.pass(SEAT_A);
    let d_spell = h2.object_of(3);
    h2.advance_native(
        SEAT_B,
        GameCommand::Cast {
            spell: d_spell,
            target: TargetRef::Player(SEAT_A),
        },
    );
    h2.pass_both();
    assert_eq!(
        h2.project_and_knowledge(uuid_b())
            .1
            .known_opponent_hand
            .len(),
        2
    );

    let f2 = h2.object_of(1);
    h2.bridged_action(uuid_a(), |scope| PlayerAction::Foretell {
        source: scope.issue(NativeReference::Object(f2)).unwrap(),
        payment: spend_colorless(2),
    });
    let (_, view_b_after_ft, _) = h2.project_and_knowledge(uuid_b());
    assert_eq!(view_b_after_ft.known_opponent_hand.len(), 1);
    assert_eq!(
        view_b_after_ft.known_opponent_hand[0].characteristics.name,
        "Decoy"
    );
    assert!(view_b_after_ft.historical_reveals.iter().any(|r| {
        r.card_name == "Secret Foretell Drake" && r.status == RevealStatusView::Invalidated
    }));
}

#[test]
fn cascade_bottomed_cards_stay_bottom_unordered_and_clear_on_shuffle() {
    let mut cascade_spell = named(
        Program::instant(
            ManaCost::one(ManaType::Green),
            ExecutableBody::new(BodyTarget::None, vec![Instruction::Jump { destination: 1 }]),
        ),
        "Cascade Bolt",
    );
    cascade_spell
        .triggered
        .push(TriggeredAbility::cascade(TriggeredAbilityId(1)));
    let free_artifact = named(
        Program::artifact(Some(ManaCost::free()), vec![]),
        "Cascaded Mox",
    );
    let shuffle = free_instant(
        "Shuffle Self",
        BodyTarget::None,
        vec![Instruction::ZoneOp(Box::new(
            spellwake_engine::toolbox::ZoneOpSpec::shuffle(
                spellwake_engine::toolbox::PlayerSelector::Resolving(ResolvingPlayer::Controller),
            ),
        ))],
    );
    // Index 0 rotates to bottom of library (after Cascaded Mox), indices 1..=7 go to hand,
    // indices 8..=10 stay on top of library so Cascade hits Zeta, Alpha, then Cascaded Mox!
    let mut h = Harness::opening_to_main(
        vec![
            (13, plain_land("Deep Bottom Land")),
            (1, cascade_spell),
            (2, plain_land("Playable Forest")),
            (3, shuffle),
            (4, plain_land("Filler 1")),
            (5, plain_land("Filler 2")),
            (6, plain_land("Filler 3")),
            (7, plain_land("Filler 4")),
            (10, plain_land("Zeta Bottom Land")),
            (11, plain_land("Alpha Bottom Land")),
            (12, free_artifact),
        ],
        10,
    );

    // Play Playable Forest and tap it for {G}, then cast Cascade Bolt.
    let land_obj = h.object_of(2);
    h.advance_native(SEAT_A, GameCommand::PlayLand { land: land_obj });
    let mana_src = h.object_of(2);
    h.bridged_action(uuid_a(), |scope| PlayerAction::ActivateMana {
        source: scope.issue(NativeReference::Object(mana_src)).unwrap(),
        ability: scope
            .issue(NativeReference::ManaAbility {
                source: mana_src,
                ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
            })
            .unwrap(),
    });
    let spell_obj = h.object_of(1);
    h.bridged_action(uuid_a(), |scope| PlayerAction::CastUntargeted {
        source: scope.issue(NativeReference::Object(spell_obj)).unwrap(),
        payment: None,
    });
    h.pass_both();

    let cascade_candidate = {
        let mut scope = h.scope(uuid_a());
        let snap = scope
            .project(
                &NativeProjectionMetadata {
                    through_event_sequence: h.seq.into(),
                    deadline_at: None,
                    disclosure: NativeDisclosure::LiveParticipant,
                },
                &h.display,
            )
            .unwrap();
        let frame = &snap.decisions[0];
        assert!(matches!(frame.domain, PlayerDomain::CascadeCast { .. }));
        let cmd = PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: scope.basis().clone(),
            action: PlayerAction::Choose {
                decision: frame.decision.clone(),
                response: PlayerChoice::CascadeCast {
                    face: None,
                    adventure: false,
                    chosen_x: None,
                    target: None,
                    payment: None,
                    additional: vec![],
                    resources: vec![],
                },
            },
        };
        scope.propose(&cmd).unwrap().into_parts().3
    };
    h.apply_candidate(cascade_candidate);

    // Both Seat A and Seat B see the two bottomed lands ONLY as `BottomUnordered`
    // sorted alphabetically ("Alpha Bottom Land", "Zeta Bottom Land"), never hidden order.
    for viewer in [uuid_a(), uuid_b()] {
        let (_, view, state) = h.project_and_knowledge(viewer);
        let bottom_names: Vec<&str> = view
            .visible_library_cards
            .iter()
            .map(|c| {
                assert_eq!(c.position, VisibleLibraryPosition::BottomUnordered);
                assert_eq!(c.source, KnowledgeSource::CascadeReveal);
                c.characteristics.name.as_str()
            })
            .collect();
        assert_eq!(bottom_names, vec!["Alpha Bottom Land", "Zeta Bottom Land"]);
        assert_eq!(state["visible_library"][0]["position"], "bottom_unordered");
        assert_eq!(state["visible_library"][1]["position"], "bottom_unordered");
    }

    h.pass_both();
    h.pass_both();
    let shuffle_obj = h.object_of(3);
    h.advance_native(
        SEAT_A,
        GameCommand::CastUntargeted {
            spell: shuffle_obj,
            payment: None,
        },
    );
    h.pass_both();

    for viewer in [uuid_a(), uuid_b()] {
        let (_, view, _) = h.project_and_knowledge(viewer);
        assert!(view.visible_library_cards.is_empty());
        assert!(view.historical_reveals.iter().any(|r| {
            r.card_name == "Alpha Bottom Land" && r.status == RevealStatusView::InvalidatedByShuffle
        }));
    }
}
