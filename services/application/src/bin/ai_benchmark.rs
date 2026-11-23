//! Native Rust AI benchmark & self-play regression harness on Spellwake's real rules engine.
//!
//! Runs full Magic: The Gathering games directly against `NativeProgramSource`,
//! `GameState`, `NativeScope`, `AiScheduler`, `compact_v1`, and `CodexAppServerProvider`
//! (the exact production Rust AI pipeline) without Python or external simulators.
use serde::Serialize;
use serde_json::json;
use spellwake_application::{
    ai_runtime::{
        AiDecisionProvider, AiModelRequest, AiScheduler, AiTier, AiTierCatalog,
        CodexAppServerConfig, CodexAppServerProvider, ScheduleDecision, StaticScriptedProvider,
        auto_tap_plan,
        compact::{
            AiActionPlan, compact_prompt_with_affordances, compact_reply_schema_for_snapshot,
        },
        diagnose_rejection,
        driver::seat_must_act,
        fallback_command, probe_priority_affordances,
    },
    native_game::{
        NativeAbilityReference, NativeBinding, NativeDisclosure, NativeDisplayScope,
        NativeDisplaySource, NativeInitialCard, NativeInitialIds, NativeInitialParticipant,
        NativeInitialSpec, NativeInitialZone, NativeObjectDisplay, NativeProgramPool,
        NativeProgramSource, NativeProjectionError, NativeProjectionMetadata, NativeScope,
        NativeSeatBinding, prepare_initial,
    },
    player_wire::{NativeRevision, PlayerAction, PlayerCommand},
};
use spellwake_card_programs::OracleId;
use spellwake_engine::{
    characteristics::ObjectCharacteristics,
    draw::GameOutcome,
    effects::DamageAllocationDomain,
    game::{GameContext, GameState},
    object_identity::GameObject,
    occurrence::{ExecutionBindings, WaitingInstance},
    priority::{PriorityPhase, SeatId},
    program::{ExecutableBody, Program},
    stack_window::{StackEntry, StackKind, WindowPhase},
    token::TokenEntryPreview,
};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use uuid::Uuid;

const MAX_RETRIES_PER_BOUNDARY: usize = 3;
const MAX_OWN_RECENT: usize = 6;

const DEFAULT_CODEX_CLI: &str = "/Users/quzzar/Projects/ai-harness-test/work/native-cli-update-recovery-001/CodexCLI.app/Contents/MacOS/codex";

// Verified Oracle IDs from work/catalogue/oracle-cards-20261029210158.jsonl.gz
const FOREST: &str = "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6";
const MOUNTAIN: &str = "a3fb7228-e76b-4e96-a40e-20b5fed75685";
const PLAINS: &str = "bc71ebf6-2056-41f7-be35-b2e5c34afa99";
const SWAMP: &str = "56719f6a-1a6c-4c0a-8d21-18f7d7350b68";
const ISLAND: &str = "b2c6aa39-2d2a-459c-a555-fb48ba993373";

const GRIZZLY_BEARS: &str = "14c8f55d-d177-4c25-a931-ebeb9e6062a0";
const ELVISH_MYSTIC: &str = "3f3b2c10-21f8-4e13-be83-4ef3fa36e123";
const GARRUKS_COMPANION: &str = "32fc8fa7-7e0a-4d4e-85cf-2f98b3fc6ecf";
const KALONIAN_TUSKER: &str = "df7f697e-6886-4897-a024-61ae225c1b34";
const LEATHERBACK_BALOTH: &str = "0ee14128-3bec-4b65-8ee4-619337d4ed45";
const GIANT_SPIDER: &str = "e740ce2f-2134-473c-afa1-1b6d2d1e38ef";
const GIANT_GROWTH: &str = "5748ebf1-24e3-499d-ab7c-c2cebd462a24";
const TITANIC_GROWTH: &str = "61e09dd9-7870-48c2-9177-d6abc3162692";

const LIGHTNING_BOLT: &str = "4457ed35-7c10-48c8-9776-456485fdf070";
const SHOCK: &str = "a9d288b8-cdc1-4e55-a0c9-d6edfc95e65d";
const LIGHTNING_STRIKE: &str = "f34b9bc4-7bfe-47fd-ba23-4eeeb46026eb";
const SEARING_SPEAR: &str = "aafd44c1-74a9-4aa7-a4a2-67eb39a07478";
const LAVA_SPIKE: &str = "2837888c-bfa8-4955-9334-0605ad409f7e";
const RAGING_GOBLIN: &str = "30997b43-fc13-41d3-8064-1ccc2cb6fd2b";
const MONASTERY_SWIFTSPEAR: &str = "dafd2713-d1bc-474b-b390-d2ff20b5375e";
const GOBLIN_GUIDE: &str = "51d9564b-44fc-4de1-9119-09d7b4089378";

const SUNTAIL_HAWK: &str = "a8d31e2f-7b2e-4135-8074-9e6ef778bd80";
const SAVANNAH_LIONS: &str = "60ba93eb-39e6-4af2-9c66-cd38f72daff2";
const ELITE_VANGUARD: &str = "b3b9a87d-cb95-435c-90b6-037406cab32e";
const STORMFRONT_PEGASUS: &str = "b35c1b30-c1dc-489f-bd79-ad6566fb9389";
const YOUTHFUL_KNIGHT: &str = "ef2a24f5-ce5e-4054-843a-2cae0c66318a";
const WHITE_KNIGHT: &str = "ddb021df-ae4a-4ac1-8353-d0b375761714";
const SERRA_ANGEL: &str = "4b7ac066-e5c7-43e6-9e7e-2739b24a905d";
const SWORDS_TO_PLOWSHARES: &str = "b1544f21-7e98-461b-aed5-e748b0168c52";
const PACIFISM: &str = "5f5e0b10-c8cf-450c-bfd3-bcb0528ec330";

const TYPHOID_RATS: &str = "d6ee6cc1-902d-4f56-afa5-6fa4813bfbbc";
const CHILD_OF_NIGHT: &str = "c650a7bc-e350-44a0-a698-d4a233d66156";
const VAMPIRE_NIGHTHAWK: &str = "feb244f8-bcb1-44cf-9940-2719221a7309";
const SENGIR_VAMPIRE: &str = "749141aa-f6c4-4ad8-b146-406e68ae9b0b";
const DOOM_BLADE: &str = "59e7f2ae-4535-4191-98be-3e65b6b2befa";
const GO_FOR_THE_THROAT: &str = "2f092562-9e17-43cd-aeb8-d0567f99363e";
const MURDER: &str = "938b4e2c-88d9-4637-bc00-e228920c9a78";
const DISFIGURE: &str = "77eafe49-b9c5-461d-89c5-cec217dd2974";
const GRASP_OF_DARKNESS: &str = "494c819d-7d73-432b-8c96-4cb9dcd31094";
const SIGN_IN_BLOOD: &str = "c6207f6a-a624-4754-88f5-dbe700c841ff";

const WIND_DRAKE: &str = "d6ffdaf0-ac08-4de9-bbce-2eab2f86bcca";
const AIR_ELEMENTAL: &str = "7744bae4-a8b7-44a5-9b4c-0048ad4cc448";
const COUNTERSPELL: &str = "cc187110-1148-4090-bbb8-e205694a39f5";
const UNSUMMON: &str = "837182db-1bf3-4a2c-bd01-1af9d9873561";
const DIVINATION: &str = "273b339c-964b-4a18-8eb5-ceb8abcdfd9e";

struct BenchDisplay<'a> {
    source: &'a NativeProgramSource,
    context: GameContext,
}

impl NativeDisplaySource for BenchDisplay<'_> {
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
        let ch = self
            .source
            .printed_card(definition)
            .map_err(NativeProjectionError::metadata)?
            .map(|c| c.characteristics().clone())
            .ok_or(NativeProjectionError::MissingDisplay)?;
        Ok(NativeObjectDisplay {
            definition_id: None,
            printing_id: None,
            characteristics: ch,
        })
    }
    fn ability(
        &self,
        _: &GameObject,
        _: &ObjectCharacteristics,
        ability: NativeAbilityReference,
    ) -> Result<String, NativeProjectionError> {
        Ok(match ability {
            NativeAbilityReference::Mana(_) => "Mana ability",
            NativeAbilityReference::Activated(_) => "Activated ability",
            NativeAbilityReference::Triggered(_) => "Triggered ability",
            NativeAbilityReference::Continuous(_) => "Static ability",
        }
        .into())
    }
    fn stack(
        &self,
        entry: &StackEntry,
        _: Option<&ExecutableBody>,
        _: Option<&ExecutionBindings>,
    ) -> Result<String, NativeProjectionError> {
        Ok(match entry.kind() {
            StackKind::CardSpell(_) => "Card spell",
            StackKind::Speed => "Increase speed",
            StackKind::Monarch => "Monarch ability",
            StackKind::DelayedCopy { .. } => "Delayed triggered ability",
            StackKind::Emblem { .. } => "Emblem ability",
            StackKind::SpellCopy { .. } => "Spell copy",
            StackKind::ActivatedAbility { .. } => "Activated ability",
            StackKind::TriggeredAbility { .. } => "Triggered ability",
        }
        .into())
    }
    fn token_preview(&self, _: &TokenEntryPreview) -> Result<String, NativeProjectionError> {
        Err(NativeProjectionError::MissingDisplay)
    }
    fn trigger(&self, _: &WaitingInstance) -> Result<String, NativeProjectionError> {
        Ok("Triggered ability".into())
    }
    fn prevention_attempt(
        &self,
        attempt: &DamageAllocationDomain,
    ) -> Result<String, NativeProjectionError> {
        Ok(format!("{} damage", String::from(attempt.amount().clone())))
    }
}

#[derive(Clone)]
struct DeckDef {
    name: &'static str,
    cards: Vec<(&'static str, &'static str, usize)>,
}

fn candidate_cards() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Forest", FOREST),
        ("Mountain", MOUNTAIN),
        ("Plains", PLAINS),
        ("Swamp", SWAMP),
        ("Island", ISLAND),
        ("Grizzly Bears", GRIZZLY_BEARS),
        ("Elvish Mystic", ELVISH_MYSTIC),
        ("Garruk's Companion", GARRUKS_COMPANION),
        ("Kalonian Tusker", KALONIAN_TUSKER),
        ("Leatherback Baloth", LEATHERBACK_BALOTH),
        ("Giant Spider", GIANT_SPIDER),
        ("Giant Growth", GIANT_GROWTH),
        ("Titanic Growth", TITANIC_GROWTH),
        ("Lightning Bolt", LIGHTNING_BOLT),
        ("Shock", SHOCK),
        ("Lightning Strike", LIGHTNING_STRIKE),
        ("Searing Spear", SEARING_SPEAR),
        ("Lava Spike", LAVA_SPIKE),
        ("Raging Goblin", RAGING_GOBLIN),
        ("Monastery Swiftspear", MONASTERY_SWIFTSPEAR),
        ("Goblin Guide", GOBLIN_GUIDE),
        ("Suntail Hawk", SUNTAIL_HAWK),
        ("Savannah Lions", SAVANNAH_LIONS),
        ("Elite Vanguard", ELITE_VANGUARD),
        ("Stormfront Pegasus", STORMFRONT_PEGASUS),
        ("Youthful Knight", YOUTHFUL_KNIGHT),
        ("White Knight", WHITE_KNIGHT),
        ("Serra Angel", SERRA_ANGEL),
        ("Swords to Plowshares", SWORDS_TO_PLOWSHARES),
        ("Pacifism", PACIFISM),
        ("Typhoid Rats", TYPHOID_RATS),
        ("Child of Night", CHILD_OF_NIGHT),
        ("Vampire Nighthawk", VAMPIRE_NIGHTHAWK),
        ("Sengir Vampire", SENGIR_VAMPIRE),
        ("Doom Blade", DOOM_BLADE),
        ("Go for the Throat", GO_FOR_THE_THROAT),
        ("Murder", MURDER),
        ("Disfigure", DISFIGURE),
        ("Grasp of Darkness", GRASP_OF_DARKNESS),
        ("Sign in Blood", SIGN_IN_BLOOD),
        ("Wind Drake", WIND_DRAKE),
        ("Air Elemental", AIR_ELEMENTAL),
        ("Counterspell", COUNTERSPELL),
        ("Unsummon", UNSUMMON),
        ("Divination", DIVINATION),
    ]
}

fn build_deck_by_name(
    name: &str,
    supported: &BTreeMap<&'static str, bool>,
) -> Result<DeckDef, String> {
    let pick = |entries: &[(&'static str, &'static str, usize)]| -> Vec<(&'static str, &'static str, usize)> {
        entries
            .iter()
            .copied()
            .filter(|(_, oid, _)| *supported.get(oid).unwrap_or(&false))
            .collect()
    };

    let raw = match name {
        "bears" => vec![("Forest", FOREST, 24), ("Grizzly Bears", GRIZZLY_BEARS, 36)],
        "green_stompy" => {
            let mut list = pick(&[
                ("Forest", FOREST, 24),
                ("Elvish Mystic", ELVISH_MYSTIC, 4),
                ("Grizzly Bears", GRIZZLY_BEARS, 4),
                ("Garruk's Companion", GARRUKS_COMPANION, 4),
                ("Kalonian Tusker", KALONIAN_TUSKER, 4),
                ("Leatherback Baloth", LEATHERBACK_BALOTH, 4),
                ("Giant Spider", GIANT_SPIDER, 4),
                ("Giant Growth", GIANT_GROWTH, 8),
                ("Titanic Growth", TITANIC_GROWTH, 4),
            ]);
            top_up_deck(&mut list, ("Grizzly Bears", GRIZZLY_BEARS), 60);
            list
        }
        "red_burn" => {
            let mut list = pick(&[
                ("Mountain", MOUNTAIN, 22),
                ("Raging Goblin", RAGING_GOBLIN, 4),
                ("Monastery Swiftspear", MONASTERY_SWIFTSPEAR, 4),
                ("Goblin Guide", GOBLIN_GUIDE, 4),
                ("Lightning Bolt", LIGHTNING_BOLT, 4),
                ("Shock", SHOCK, 4),
                ("Lightning Strike", LIGHTNING_STRIKE, 4),
                ("Searing Spear", SEARING_SPEAR, 4),
                ("Lava Spike", LAVA_SPIKE, 4),
            ]);
            let fallback = if *supported.get(SHOCK).unwrap_or(&false) {
                ("Shock", SHOCK)
            } else {
                ("Raging Goblin", RAGING_GOBLIN)
            };
            top_up_deck(&mut list, fallback, 60);
            list
        }
        "white_flyers" => {
            let mut list = pick(&[
                ("Plains", PLAINS, 24),
                ("Suntail Hawk", SUNTAIL_HAWK, 4),
                ("Savannah Lions", SAVANNAH_LIONS, 4),
                ("Elite Vanguard", ELITE_VANGUARD, 4),
                ("Stormfront Pegasus", STORMFRONT_PEGASUS, 4),
                ("Youthful Knight", YOUTHFUL_KNIGHT, 4),
                ("White Knight", WHITE_KNIGHT, 4),
                ("Serra Angel", SERRA_ANGEL, 4),
                ("Swords to Plowshares", SWORDS_TO_PLOWSHARES, 4),
                ("Pacifism", PACIFISM, 4),
            ]);
            top_up_deck(&mut list, ("Stormfront Pegasus", STORMFRONT_PEGASUS), 60);
            list
        }
        "black_midrange" => {
            let mut list = pick(&[
                ("Swamp", SWAMP, 24),
                ("Typhoid Rats", TYPHOID_RATS, 4),
                ("Child of Night", CHILD_OF_NIGHT, 4),
                ("Vampire Nighthawk", VAMPIRE_NIGHTHAWK, 4),
                ("Sengir Vampire", SENGIR_VAMPIRE, 4),
                ("Doom Blade", DOOM_BLADE, 4),
                ("Go for the Throat", GO_FOR_THE_THROAT, 4),
                ("Murder", MURDER, 4),
                ("Disfigure", DISFIGURE, 4),
                ("Grasp of Darkness", GRASP_OF_DARKNESS, 4),
            ]);
            top_up_deck(&mut list, ("Child of Night", CHILD_OF_NIGHT), 60);
            list
        }
        "blue_tempo" => {
            let mut list = pick(&[
                ("Island", ISLAND, 24),
                ("Wind Drake", WIND_DRAKE, 8),
                ("Air Elemental", AIR_ELEMENTAL, 4),
                ("Counterspell", COUNTERSPELL, 8),
                ("Unsummon", UNSUMMON, 8),
                ("Divination", DIVINATION, 8),
            ]);
            top_up_deck(&mut list, ("Wind Drake", WIND_DRAKE), 60);
            list
        }
        other => return Err(format!("unknown deck '{other}'")),
    };

    Ok(DeckDef {
        name: Box::leak(name.to_owned().into_boxed_str()),
        cards: raw,
    })
}

fn top_up_deck(
    list: &mut Vec<(&'static str, &'static str, usize)>,
    filler: (&'static str, &'static str),
    target: usize,
) {
    let total: usize = list.iter().map(|(_, _, n)| *n).sum();
    if total < target {
        list.push((filler.0, filler.1, target - total));
    }
}

fn to_initial_cards(deck: &DeckDef) -> Vec<NativeInitialCard> {
    let mut out = Vec::new();
    for &(_, oid, count) in &deck.cards {
        let oracle_id = OracleId::try_from(oid.to_owned()).expect("valid oracle id");
        for _ in 0..count {
            out.push(NativeInitialCard {
                oracle_id: oracle_id.clone(),
                zone: NativeInitialZone::Library,
            });
        }
    }
    out
}

fn probe_card_support(source: &NativeProgramSource) -> BTreeMap<&'static str, bool> {
    let mut out = BTreeMap::new();
    let forest = OracleId::try_from(FOREST.to_owned()).expect("valid forest id");
    for &(_name, oid) in &candidate_cards() {
        let card_oid = OracleId::try_from(oid.to_owned()).expect("valid oracle id");
        let mut deck = vec![NativeInitialCard {
            oracle_id: card_oid,
            zone: NativeInitialZone::Library,
        }];
        for _ in 0..59 {
            deck.push(NativeInitialCard {
                oracle_id: forest.clone(),
                zone: NativeInitialZone::Library,
            });
        }
        let res = prepare_initial(
            NativeInitialIds {
                game_id: Uuid::new_v4(),
                context_id: Uuid::new_v4(),
                release_id: Uuid::from_u128(9003),
                profile_id: Uuid::from_u128(9004),
            },
            source,
            NativeInitialSpec {
                first_player_seat_id: seat_uuid(0),
                participants: vec![
                    NativeInitialParticipant {
                        seat_id: seat_uuid(0),
                        life: 20.into(),
                        cards: deck,
                    },
                    NativeInitialParticipant {
                        seat_id: seat_uuid(1),
                        life: 20.into(),
                        cards: (0..60)
                            .map(|_| NativeInitialCard {
                                oracle_id: forest.clone(),
                                zone: NativeInitialZone::Library,
                            })
                            .collect(),
                    },
                ],
                entropy: make_entropy(42),
            },
        );
        out.insert(oid, res.is_ok());
    }
    out
}

fn seat_uuid(index: usize) -> Uuid {
    Uuid::from_u128(101 + index as u128)
}

fn make_entropy(seed: u64) -> Vec<NativeRevision> {
    let mut x = seed;
    (0..2048)
        .map(|_| {
            x = x.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            (z ^ (z >> 31)).into()
        })
        .collect()
}

fn binding_for(state: &GameState, context_id: Uuid, viewer: Uuid) -> NativeBinding {
    NativeBinding::new(
        context_id,
        state.basis().clone(),
        vec![
            NativeSeatBinding {
                seat_id: seat_uuid(0),
                native_seat: SeatId(0),
            },
            NativeSeatBinding {
                seat_id: seat_uuid(1),
                native_seat: SeatId(1),
            },
        ],
        viewer,
    )
    .expect("valid binding")
}

fn current_actor(state: &GameState) -> Option<Uuid> {
    if state.outcome().is_some() {
        return None;
    }
    let native = if let Some(d) = state.pending_decision() {
        d.chooser()
    } else if let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
        state.execution().window().phase()
    {
        holder
    } else {
        return None;
    };
    match native {
        SeatId(0) => Some(seat_uuid(0)),
        SeatId(1) => Some(seat_uuid(1)),
        _ => None,
    }
}

#[derive(Default, Clone, Serialize)]
struct SeatStats {
    tier: String,
    deck: String,
    model: String,
    boundaries: u64,
    llm_calls: u64,
    delegated_passes: u64,
    fast_passes: u64,
    first_try_accepts: u64,
    codec_rejections: u64,
    engine_rejections: u64,
    provider_errors: u64,
    fallback_actions: u64,
    exhausted_boundaries: u64,
    latencies_ms: Vec<u64>,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
    rejection_reasons: BTreeMap<String, usize>,
}

impl SeatStats {
    fn summary(&self) -> SeatSummary {
        let mut sorted = self.latencies_ms.clone();
        sorted.sort_unstable();
        let mean = if sorted.is_empty() {
            0
        } else {
            sorted.iter().sum::<u64>() / sorted.len() as u64
        };
        let p50 = percentile(&sorted, 50);
        let p95 = percentile(&sorted, 95);
        let max = sorted.last().copied().unwrap_or(0);
        let total_ai_ms: u64 = sorted.iter().sum();
        let cache_hit_pct = if self.input_tokens == 0 {
            0.0
        } else {
            (self.cached_input_tokens as f64 / self.input_tokens as f64) * 100.0
        };
        let attempted = self
            .boundaries
            .saturating_sub(self.delegated_passes + self.fast_passes);
        let first_try_pct = if attempted == 0 {
            100.0
        } else {
            (self.first_try_accepts as f64 / attempted as f64) * 100.0
        };
        SeatSummary {
            tier: self.tier.clone(),
            deck: self.deck.clone(),
            model: self.model.clone(),
            boundaries: self.boundaries,
            llm_calls: self.llm_calls,
            delegated_passes: self.delegated_passes,
            fast_passes: self.fast_passes,
            first_try_accepts: self.first_try_accepts,
            first_try_pct,
            codec_rejections: self.codec_rejections,
            engine_rejections: self.engine_rejections,
            provider_errors: self.provider_errors,
            fallback_actions: self.fallback_actions,
            exhausted_boundaries: self.exhausted_boundaries,
            latency_mean_ms: mean,
            latency_p50_ms: p50,
            latency_p95_ms: p95,
            latency_max_ms: max,
            total_ai_ms,
            input_tokens: self.input_tokens,
            cached_input_tokens: self.cached_input_tokens,
            cache_hit_pct,
            output_tokens: self.output_tokens,
            reasoning_tokens: self.reasoning_tokens,
            rejection_reasons: self.rejection_reasons.clone(),
        }
    }
}

#[derive(Clone, Serialize)]
struct SeatSummary {
    tier: String,
    deck: String,
    model: String,
    boundaries: u64,
    llm_calls: u64,
    delegated_passes: u64,
    fast_passes: u64,
    first_try_accepts: u64,
    first_try_pct: f64,
    codec_rejections: u64,
    engine_rejections: u64,
    provider_errors: u64,
    fallback_actions: u64,
    exhausted_boundaries: u64,
    latency_mean_ms: u64,
    latency_p50_ms: u64,
    latency_p95_ms: u64,
    latency_max_ms: u64,
    total_ai_ms: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_hit_pct: f64,
    output_tokens: u64,
    reasoning_tokens: u64,
    rejection_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Serialize)]
struct GameReport {
    game_index: usize,
    seed: u64,
    matchup: String,
    winner_seat: Option<usize>,
    winner_tier: Option<String>,
    termination: String,
    turns: u32,
    steps: u32,
    final_life: [String; 2],
    wall_ms: u64,
    seats: [SeatSummary; 2],
}

fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) * pct) / 100;
    sorted[idx]
}

fn load_source() -> Result<NativeProgramSource, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| format!("failed to locate repo root: {e}"))?;
    let metadata = fs::read(root.join("work/catalogue/oracle-metadata.json"))
        .map_err(|e| format!("missing oracle-metadata.json: {e}"))?;
    let snapshot = fs::read(root.join("work/catalogue/oracle-cards-20261029210158.jsonl.gz"))
        .map_err(|e| format!("missing oracle-cards snapshot: {e}"))?;
    let vocab = fs::read(root.join(
        "work/resolution-value-choices/current-s1-packets-v1/creature-vocabulary.input.v1.json",
    ))
    .map_err(|e| format!("missing creature vocabulary: {e}"))?;
    NativeProgramSource::from_bytes(
        NativeProgramPool::KeywordGameplay,
        &metadata,
        snapshot,
        &vocab,
    )
    .map_err(|e| format!("failed to load native program source: {e}"))
}

fn summarize_action(plan: &AiActionPlan) -> String {
    match plan {
        AiActionPlan::Single(cmd) => match &cmd.action {
            PlayerAction::Pass {} => "pass".to_owned(),
            PlayerAction::PlayLand { .. } => "play-land".to_owned(),
            PlayerAction::ActivateMana { .. } => "activate-mana".to_owned(),
            PlayerAction::CastUntargeted { .. } => "cast-untargeted".to_owned(),
            PlayerAction::CastTargeted { .. } => "cast-targeted".to_owned(),
            PlayerAction::ActivateAbility { .. } => "activate-ability".to_owned(),
            PlayerAction::ActivateUntargetedAbility { .. } => {
                "activate-untargeted-ability".to_owned()
            }
            PlayerAction::Choose { .. } => "choose".to_owned(),
            _ => "other".to_owned(),
        },
        AiActionPlan::CastBatch(cmds) => format!("cast-batch({} steps)", cmds.len()),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_game(
    game_index: usize,
    seed: u64,
    source: &NativeProgramSource,
    provider: &Arc<dyn AiDecisionProvider>,
    decks: [&DeckDef; 2],
    tiers: [AiTier; 2],
    max_turns: u32,
    max_steps: u32,
    log_path: Option<&Path>,
) -> Result<GameReport, String> {
    let started = Instant::now();
    let game_id = Uuid::new_v4();
    let context_id = Uuid::new_v4();
    let opening = prepare_initial(
        NativeInitialIds {
            game_id,
            context_id,
            release_id: Uuid::from_u128(9003),
            profile_id: Uuid::from_u128(9004),
        },
        source,
        NativeInitialSpec {
            first_player_seat_id: seat_uuid(0),
            participants: vec![
                NativeInitialParticipant {
                    seat_id: seat_uuid(0),
                    life: 20.into(),
                    cards: to_initial_cards(decks[0]),
                },
                NativeInitialParticipant {
                    seat_id: seat_uuid(1),
                    life: 20.into(),
                    cards: to_initial_cards(decks[1]),
                },
            ],
            entropy: make_entropy(seed),
        },
    )
    .map_err(|e| format!("prepare_initial failed: {e}"))?;

    let (_, _, candidate, _) = opening.into_parts();
    let mut state = candidate.into_state();
    let context = state.basis().context.clone();
    let display = BenchDisplay { source, context };

    let mut schedulers = [AiScheduler::new(), AiScheduler::new()];
    let mut recents: [VecDeque<String>; 2] = [VecDeque::new(), VecDeque::new()];
    let mut stats = [
        SeatStats {
            tier: tiers[0].as_str().to_owned(),
            deck: decks[0].name.to_owned(),
            ..Default::default()
        },
        SeatStats {
            tier: tiers[1].as_str().to_owned(),
            deck: decks[1].name.to_owned(),
            ..Default::default()
        },
    ];
    let mut transcript = Vec::<String>::new();
    let mut seq = 0_u64;
    let mut steps = 0_u32;
    let mut last_turn = 0_u32;
    let mut termination = "completed".to_owned();
    let mut final_life = ["20".to_owned(), "20".to_owned()];

    while steps < max_steps {
        if let Some(outcome) = state.outcome() {
            termination = match outcome {
                GameOutcome::Winner(SeatId(0)) => "winner_seat_0".to_owned(),
                GameOutcome::Winner(SeatId(1)) => "winner_seat_1".to_owned(),
                GameOutcome::Winner(_) => "winner_other".to_owned(),
                GameOutcome::Draw => "draw".to_owned(),
            };
            break;
        }
        let Some(actor) = current_actor(&state) else {
            termination = "no_actor".to_owned();
            break;
        };
        let seat_idx = usize::from(actor == seat_uuid(1));
        let mut scope = NativeScope::new(&state, binding_for(&state, context_id, actor))
            .map_err(|e| format!("scope failed: {e}"))?;
        let meta = NativeProjectionMetadata {
            through_event_sequence: seq.into(),
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        let snap = scope
            .project(&meta, &display)
            .map_err(|e| format!("project failed: {e}"))?;

        for p in &snap.players {
            let idx = usize::from(p.seat_id == seat_uuid(1));
            final_life[idx] = String::from(p.life.clone());
        }
        if let Some(t) = &snap.turn {
            let tnum: u32 = String::from(t.number.clone()).parse().unwrap_or(0);
            last_turn = last_turn.max(tnum);
            if tnum > max_turns {
                termination = "max_turns".to_owned();
                break;
            }
        }
        if !seat_must_act(&snap) {
            termination = "not_must_act".to_owned();
            break;
        }

        stats[seat_idx].boundaries += 1;
        steps += 1;

        let turn_str = snap
            .turn
            .as_ref()
            .map(|t| {
                let p = serde_json::to_string(&t.phase).unwrap_or_default();
                let s = serde_json::to_string(&t.step).unwrap_or_default();
                format!(
                    "T{} {}/{}",
                    String::from(t.number.clone()),
                    p.trim_matches('"'),
                    s.trim_matches('"'),
                )
            })
            .unwrap_or_else(|| "PreGame".to_owned());

        let affordances = probe_priority_affordances(&scope, &snap);

        // Deterministic <1 ms fast-pass when holding priority with zero legal non-pass plays.
        if snap.decisions.is_empty() && !affordances.has_non_pass_play() {
            let cmd = PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: snap.basis.clone(),
                action: PlayerAction::Pass {},
            };
            if let Ok(prepared) = scope.propose(&cmd) {
                state = prepared.into_parts().3.into_state();
                seq += 1;
                stats[seat_idx].fast_passes += 1;
                transcript.push(format!(
                    "[step={steps:04} {turn_str} S{seat_idx}({}) L={}/{}] FAST_PASS (0 legal plays)",
                    tiers[seat_idx].as_str(),
                    final_life[0],
                    final_life[1],
                ));
                continue;
            }
        }

        match schedulers[seat_idx].evaluate(&snap) {
            ScheduleDecision::DelegatePass => {
                let cmd = PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: snap.basis.clone(),
                    action: PlayerAction::Pass {},
                };
                match scope.propose(&cmd) {
                    Ok(prepared) => {
                        state = prepared.into_parts().3.into_state();
                        seq += 1;
                        schedulers[seat_idx].record_delegated_pass();
                        stats[seat_idx].delegated_passes += 1;
                        transcript.push(format!(
                            "[step={steps:04} {turn_str} S{seat_idx}({}) L={}/{}] DELEGATED pass",
                            tiers[seat_idx].as_str(),
                            final_life[0],
                            final_life[1],
                        ));
                    }
                    Err(err) => {
                        schedulers[seat_idx].revoke();
                        transcript.push(format!(
                            "[step={steps:04} {turn_str} S{seat_idx}] delegated pass rejected ({err}), revoking grant"
                        ));
                    }
                }
            }
            ScheduleDecision::CallModel(reason) => {
                let schema = compact_reply_schema_for_snapshot(&snap);
                let cache_key =
                    format!("bench:{}:{}:{seat_idx}", tiers[seat_idx].as_str(), game_id);
                let own_recent: Vec<String> = recents[seat_idx].iter().cloned().collect();
                let mut feedback: Option<String> = None;
                let mut accepted_state: Option<(
                    GameState,
                    String,
                    bool,
                    std::collections::BTreeSet<spellwake_application::ai_runtime::WakeCondition>,
                    u32,
                    String,
                )> = None;

                for attempt in 0..MAX_RETRIES_PER_BOUNDARY {
                    let (prompt, codec) = compact_prompt_with_affordances(
                        &snap,
                        &affordances,
                        &own_recent,
                        feedback.as_deref(),
                    )
                    .map_err(|e| format!("compact_prompt failed: {e}"))?;
                    let resp = match provider.decide(&AiModelRequest {
                        tier: tiers[seat_idx],
                        game_id,
                        seat_id: actor,
                        prompt,
                        schema: schema.clone(),
                        cache_key: cache_key.clone(),
                    }) {
                        Ok(r) => r,
                        Err(err) => {
                            stats[seat_idx].provider_errors += 1;
                            *stats[seat_idx]
                                .rejection_reasons
                                .entry(format!("provider: {err}"))
                                .or_insert(0) += 1;
                            transcript.push(format!(
                                "[step={steps:04} {turn_str} S{seat_idx} attempt={attempt}] PROVIDER_ERR: {err}"
                            ));
                            if !err.is_retryable() {
                                break;
                            }
                            continue;
                        }
                    };
                    stats[seat_idx].llm_calls += 1;
                    stats[seat_idx].model = resp.model.clone();
                    stats[seat_idx].latencies_ms.push(resp.latency_ms);
                    stats[seat_idx].input_tokens += resp.usage.input_tokens;
                    stats[seat_idx].cached_input_tokens += resp.usage.cached_input_tokens;
                    stats[seat_idx].output_tokens += resp.usage.output_tokens;
                    stats[seat_idx].reasoning_tokens += resp.usage.reasoning_tokens;

                    let expanded = match codec.expand_reply(&resp.raw_json, &snap.basis) {
                        Ok(e) => e,
                        Err(err) => {
                            stats[seat_idx].codec_rejections += 1;
                            *stats[seat_idx]
                                .rejection_reasons
                                .entry(format!("codec: {err}"))
                                .or_insert(0) += 1;
                            feedback =
                                Some(format!("previous reply rejected before commit: {err}"));
                            transcript.push(format!(
                                "[step={steps:04} {turn_str} S{seat_idx} attempt={attempt} {}ms] CODEC_REJECT ({err}) raw={}",
                                resp.latency_ms, resp.raw_json
                            ));
                            continue;
                        }
                    };

                    let plan =
                        auto_tap_plan(expanded.plan, &snap, &affordances.mana_sources, &snap.basis);

                    let commit_res: Result<GameState, String> = match &plan {
                        AiActionPlan::Single(cmd) => scope
                            .propose(cmd)
                            .map(|p| p.into_parts().3.into_state())
                            .map_err(|e| e.to_string()),
                        AiActionPlan::CastBatch(cmds) => scope
                            .prepare_cast_batch(cmds)
                            .map(|p| p.into_parts().0)
                            .map_err(|e| e.to_string()),
                    };

                    match commit_res {
                        Ok(next_state) => {
                            if attempt == 0 {
                                stats[seat_idx].first_try_accepts += 1;
                            }
                            let summary = summarize_action(&plan);
                            let line = format!(
                                "[step={steps:04} {turn_str} S{seat_idx}({}:{}) wake={reason:?} L={}/{} {}ms in={}/c={}] {} | raw={}",
                                tiers[seat_idx].as_str(),
                                decks[seat_idx].name,
                                final_life[0],
                                final_life[1],
                                resp.latency_ms,
                                resp.usage.input_tokens,
                                resp.usage.cached_input_tokens,
                                summary,
                                resp.raw_json
                            );
                            accepted_state = Some((
                                next_state,
                                summary,
                                expanded.delegate_pass,
                                expanded.wake,
                                expanded.max_skips,
                                line,
                            ));
                            break;
                        }
                        Err(err) => {
                            stats[seat_idx].engine_rejections += 1;
                            *stats[seat_idx]
                                .rejection_reasons
                                .entry(format!("engine: {err}"))
                                .or_insert(0) += 1;
                            feedback = Some(diagnose_rejection(&plan, &affordances, &codec, &err));
                            transcript.push(format!(
                                "[step={steps:04} {turn_str} S{seat_idx} attempt={attempt} {}ms] ENGINE_REJECT ({err}) raw={}",
                                resp.latency_ms, resp.raw_json
                            ));
                        }
                    }
                }

                if let Some((next_state, summary, delegate_pass, wake, max_skips, line)) =
                    accepted_state
                {
                    drop(scope);
                    state = next_state;
                    seq += 1;
                    if recents[seat_idx].len() >= MAX_OWN_RECENT {
                        recents[seat_idx].pop_front();
                    }
                    recents[seat_idx].push_back(summary);
                    let post_snap =
                        NativeScope::new(&state, binding_for(&state, context_id, actor))
                            .ok()
                            .and_then(|mut s| {
                                let pm = NativeProjectionMetadata {
                                    through_event_sequence: seq.into(),
                                    deadline_at: None,
                                    disclosure: NativeDisclosure::LiveParticipant,
                                };
                                s.project(&pm, &display).ok()
                            });
                    if let Some(post) = post_snap {
                        schedulers[seat_idx].record_model_acceptance(
                            delegate_pass,
                            wake,
                            max_skips,
                            &post,
                        );
                    } else {
                        schedulers[seat_idx].revoke();
                    }
                    eprintln!("{line}");
                    transcript.push(line);
                } else {
                    stats[seat_idx].exhausted_boundaries += 1;
                    let fb_next = fallback_command(&snap)
                        .and_then(|cmd| scope.propose(&cmd).ok())
                        .map(|p| p.into_parts().3.into_state());
                    drop(scope);
                    if let Some(next_state) = fb_next {
                        state = next_state;
                        seq += 1;
                        schedulers[seat_idx].revoke();
                        stats[seat_idx].fallback_actions += 1;
                        transcript.push(format!(
                            "[step={steps:04} {turn_str} S{seat_idx}] EXHAUSTED retries -> SAFE_FALLBACK"
                        ));
                        continue;
                    }
                    termination = format!("stuck_seat_{seat_idx}_at_{turn_str}");
                    transcript.push(format!(
                        "[step={steps:04} {turn_str} S{seat_idx}] EXHAUSTED {MAX_RETRIES_PER_BOUNDARY} retries -> STUCK"
                    ));
                    break;
                }
            }
        }
    }

    if steps >= max_steps && termination == "completed" {
        termination = "max_steps".to_owned();
    }

    let winner_seat = match state.outcome() {
        Some(GameOutcome::Winner(SeatId(0))) => Some(0),
        Some(GameOutcome::Winner(SeatId(1))) => Some(1),
        _ => None,
    };
    let winner_tier = winner_seat.map(|i| tiers[i].as_str().to_owned());

    if let Some(path) = log_path {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, transcript.join("\n"));
    }

    Ok(GameReport {
        game_index,
        seed,
        matchup: format!(
            "{}({}) vs {}({})",
            decks[0].name,
            tiers[0].as_str(),
            decks[1].name,
            tiers[1].as_str()
        ),
        winner_seat,
        winner_tier,
        termination,
        turns: last_turn,
        steps,
        final_life,
        wall_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        seats: [stats[0].summary(), stats[1].summary()],
    })
}

fn parse_tier(raw: &str) -> Result<AiTier, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "normal" => Ok(AiTier::Normal),
        "advanced" => Ok(AiTier::Advanced),
        other => Err(format!("invalid tier '{other}' (expected normal|advanced)")),
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let mut probe_decks = false;
    let mut deck0_name = "red_burn".to_owned();
    let mut deck1_name = "green_stompy".to_owned();
    let mut tier0 = AiTier::Advanced;
    let mut tier1 = AiTier::Normal;
    let mut games: usize = 2;
    let mut swap_sides = true;
    let mut base_seed: u64 = 2026112301;
    let mut max_turns: u32 = 20;
    let mut max_steps: u32 = 1200;
    let mut out_dir: Option<PathBuf> = None;
    let mut scripted = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--probe-decks" => probe_decks = true,
            "--scripted" => scripted = true,
            "--no-swap" => swap_sides = false,
            "--deck0" => {
                i += 1;
                deck0_name = args.get(i).cloned().ok_or("--deck0 needs value")?;
            }
            "--deck1" => {
                i += 1;
                deck1_name = args.get(i).cloned().ok_or("--deck1 needs value")?;
            }
            "--tier0" => {
                i += 1;
                tier0 = parse_tier(args.get(i).ok_or("--tier0 needs value")?)?;
            }
            "--tier1" => {
                i += 1;
                tier1 = parse_tier(args.get(i).ok_or("--tier1 needs value")?)?;
            }
            "--games" => {
                i += 1;
                games = args
                    .get(i)
                    .ok_or("--games needs value")?
                    .parse()
                    .map_err(|_| "invalid --games")?;
            }
            "--seed" => {
                i += 1;
                base_seed = args
                    .get(i)
                    .ok_or("--seed needs value")?
                    .parse()
                    .map_err(|_| "invalid --seed")?;
            }
            "--max-turns" => {
                i += 1;
                max_turns = args
                    .get(i)
                    .ok_or("--max-turns needs value")?
                    .parse()
                    .map_err(|_| "invalid --max-turns")?;
            }
            "--max-steps" => {
                i += 1;
                max_steps = args
                    .get(i)
                    .ok_or("--max-steps needs value")?
                    .parse()
                    .map_err(|_| "invalid --max-steps")?;
            }
            "--out" => {
                i += 1;
                out_dir = Some(PathBuf::from(args.get(i).ok_or("--out needs value")?));
            }
            other => return Err(format!("unknown arg: {other}")),
        }
        i += 1;
    }

    let source = load_source()?;
    let supported = probe_card_support(&source);

    if probe_decks {
        println!("=== Card Support in NativeProgramPool::ResolutionSelectionNaturalMana ===");
        let mut ok_count = 0;
        for &(name, oid) in &candidate_cards() {
            let is_ok = *supported.get(oid).unwrap_or(&false);
            if is_ok {
                ok_count += 1;
            }
            println!(
                "{:<24} : {}",
                name,
                if is_ok { "SUPPORTED" } else { "unsupported" }
            );
        }
        println!("Total supported: {ok_count}/{}", candidate_cards().len());
        for dname in [
            "bears",
            "red_burn",
            "green_stompy",
            "white_flyers",
            "black_midrange",
            "blue_tempo",
        ] {
            let d = build_deck_by_name(dname, &supported)?;
            let summary: Vec<String> = d
                .cards
                .iter()
                .map(|(n, _, c)| format!("{c}x {n}"))
                .collect();
            println!("Deck {dname}: {}", summary.join(", "));
        }
        return Ok(());
    }

    let d0 = build_deck_by_name(&deck0_name, &supported)?;
    let d1 = build_deck_by_name(&deck1_name, &supported)?;

    let provider: Arc<dyn AiDecisionProvider> = if scripted {
        let keep = json!({
            "action": {"kind": "choose", "decision": "d1", "response": {"kind": "option", "option": "opt1"}},
            "delegate_pass": false, "wake": [], "max_skips": 0
        })
        .to_string();
        let pass = json!({
            "action": {"kind": "pass"},
            "delegate_pass": true, "wake": ["own_main"], "max_skips": 8
        })
        .to_string();
        Arc::new(StaticScriptedProvider::new(vec![keep.clone(), keep], pass))
    } else {
        let cli = std::env::var("SPELLWAKE_AI_CODEX_CLI")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CODEX_CLI));
        let app_srv = CodexAppServerProvider::new(CodexAppServerConfig {
            cli_path: cli,
            codex_home: None,
            seed_auth_json: None,
            catalog: AiTierCatalog::from_env(),
            concurrency: 2,
        })
        .map_err(|e| format!("failed to init CodexAppServerProvider: {e}"))?;
        app_srv
            .verify_chatgpt_login()
            .map_err(|e| format!("ChatGPT login verification failed: {e}"))?;
        Arc::new(app_srv)
    };

    let mut reports = Vec::new();
    for g in 0..games {
        let swapped = swap_sides && (g % 2 == 1);
        let (decks, tiers) = if swapped {
            ([&d1, &d0], [tier1, tier0])
        } else {
            ([&d0, &d1], [tier0, tier1])
        };
        let seed = base_seed + (g as u64) * 97;
        let log_path = out_dir.as_ref().map(|d| d.join(format!("game-{g}.log")));
        eprintln!(
            "\n=== Starting Game {g}: {}({}) vs {}({}) seed={seed} ===",
            decks[0].name,
            tiers[0].as_str(),
            decks[1].name,
            tiers[1].as_str()
        );
        let rep = run_game(
            g,
            seed,
            &source,
            &provider,
            decks,
            tiers,
            max_turns,
            max_steps,
            log_path.as_deref(),
        )?;
        eprintln!(
            "=== Finished Game {g}: term={} winner={:?} turns={} life={:?} wall={}ms ===",
            rep.termination, rep.winner_tier, rep.turns, rep.final_life, rep.wall_ms
        );
        reports.push(rep);
    }

    let summary_json = serde_json::to_string_pretty(&reports)
        .map_err(|e| format!("serialize report failed: {e}"))?;
    if let Some(dir) = &out_dir {
        fs::create_dir_all(dir).map_err(|e| format!("create out dir failed: {e}"))?;
        fs::write(dir.join("summary.json"), &summary_json)
            .map_err(|e| format!("write summary.json failed: {e}"))?;
    }
    println!("{summary_json}");
    Ok(())
}
