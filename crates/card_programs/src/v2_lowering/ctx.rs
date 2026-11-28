//! Shared lowering context and executable-body accumulator for `v2_lowering`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};
use spellwake_engine::{
    activated::ActivatedAbilityId,
    characteristics::StaticAbilityId,
    land::LandType,
    mana::ManaAbilityId,
    program::{
        ArtifactSubtype, BodyTarget, EnchantmentSubtype, EventValueKind, ExecutableBody,
        Instruction, PlaneswalkerSubtype, ResolvingPlayer, SpellSubtype, TriggeredAbilityId,
        ZoneReplacementAbilityId,
    },
    toolbox::{BindingId, TagKey},
};
use spellwake_oracle_compiler::{Vocabulary as OracleVocabulary, v2::ir::TargetCount};

use crate::v2_lowering::error::{LowerResult, LoweringError};

/// Cached [`OracleVocabulary`] keyed by [`CreatureTypeVocabulary`] identifier.
#[must_use]
pub(crate) fn cached_oracle_vocab(vocab: &CreatureTypeVocabulary) -> Arc<OracleVocabulary> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, Arc<OracleVocabulary>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let key = vocab.id().to_string();
    if let Ok(guard) = cache.lock()
        && let Some(hit) = guard.get(&key)
    {
        return Arc::clone(hit);
    }
    let built = Arc::new(OracleVocabulary::from_creature_members(vocab.members()));
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, Arc::clone(&built));
    }
    built
}

/// Mutable per-face lowering context allocating deterministic ability IDs and `ExecEnv` slots.
pub(crate) struct LowerCtx<'a> {
    pub vocab: &'a CreatureTypeVocabulary,
    pub face_name: &'a str,
    pub next_mana_id: u64,
    pub next_activated_id: u64,
    pub next_triggered_id: u64,
    pub next_static_id: u64,
    pub next_replacement_id: u64,
    pub next_binding_slot: u16,
    pub in_trigger: bool,
    pub trigger_has_participant: bool,
    pub trigger_is_spell_cast: bool,
    pub trigger_damage_event: Option<EventValueKind>,
    pub has_object_target: bool,
    pub last_player_target: Option<ResolvingPlayer>,
    pub in_loop_player: bool,
    pub in_loop_object: Option<BindingId>,
    pub bound_x_slot: Option<BindingId>,
    pub last_moved_binding: Option<BindingId>,
    pub last_unchosen_binding: Option<BindingId>,
    pub last_succeeded_binding: Option<BindingId>,
    pub self_antecedent: bool,
    pub delayed_tag_key: Option<TagKey>,
    pub is_spell_body: bool,
    pub last_damage_was_self: bool,
}

impl<'a> LowerCtx<'a> {
    #[must_use]
    pub fn new(vocab: &'a CreatureTypeVocabulary, _card_name: &'a str, face_name: &'a str) -> Self {
        Self {
            vocab,
            face_name,
            next_mana_id: 1_000,
            next_activated_id: 1_000,
            next_triggered_id: 1_000,
            next_static_id: 1_000,
            next_replacement_id: 1_000,
            next_binding_slot: 1,
            in_trigger: false,
            trigger_has_participant: true,
            trigger_is_spell_cast: false,
            trigger_damage_event: None,
            has_object_target: false,
            last_player_target: None,
            in_loop_player: false,
            in_loop_object: None,
            bound_x_slot: None,
            last_moved_binding: None,
            last_unchosen_binding: None,
            last_succeeded_binding: None,
            self_antecedent: false,
            delayed_tag_key: None,
            is_spell_body: false,
            last_damage_was_self: false,
        }
    }

    fn reset_body_target_state(&mut self) {
        self.has_object_target = false;
        self.last_player_target = None;
        self.trigger_damage_event = None;
        self.self_antecedent = false;
        self.delayed_tag_key = None;
        self.last_damage_was_self = false;
    }

    pub fn alloc_mana_id(&mut self) -> ManaAbilityId {
        self.reset_body_target_state();
        let id = ManaAbilityId(self.next_mana_id);
        self.next_mana_id += 1;
        id
    }

    pub fn alloc_activated_id(&mut self) -> ActivatedAbilityId {
        self.reset_body_target_state();
        let id = ActivatedAbilityId(self.next_activated_id);
        self.next_activated_id += 1;
        id
    }

    pub fn alloc_triggered_id(&mut self) -> TriggeredAbilityId {
        self.reset_body_target_state();
        let id = TriggeredAbilityId(self.next_triggered_id);
        self.next_triggered_id += 1;
        id
    }

    pub fn alloc_static_id(&mut self) -> StaticAbilityId {
        self.reset_body_target_state();
        let id = StaticAbilityId(self.next_static_id);
        self.next_static_id += 1;
        id
    }

    pub fn alloc_replacement_id(&mut self) -> ZoneReplacementAbilityId {
        self.reset_body_target_state();
        let id = ZoneReplacementAbilityId(self.next_replacement_id);
        self.next_replacement_id += 1;
        id
    }

    pub fn alloc_binding(&mut self) -> LowerResult<BindingId> {
        if self.next_binding_slot >= u16::MAX - 16 {
            return Err(LoweringError::Unsupported("v2:binding-slot-overflow"));
        }
        let id = BindingId(self.next_binding_slot);
        self.next_binding_slot += 1;
        Ok(id)
    }

    pub fn lookup_creature_type(&self, raw: &str) -> Option<CreatureTypeId> {
        if let Some(id) = self.vocab.resolve_label(raw) {
            return Some(id);
        }
        let mut chars = raw.chars();
        let first = chars.next()?;
        let mut titled = String::with_capacity(raw.len());
        titled.extend(first.to_uppercase());
        titled.push_str(&chars.as_str().to_ascii_lowercase());
        if let Some(id) = self.vocab.resolve_label(&titled) {
            return Some(id);
        }
        self.vocab
            .members()
            .iter()
            .find(|m| m.eq_ignore_ascii_case(raw))
            .and_then(|m| self.vocab.resolve_label(m))
    }

    #[must_use]
    pub fn lookup_land_type(raw: &str) -> Option<LandType> {
        let lower = raw.to_ascii_lowercase();
        match lower.as_str() {
            "plains" => Some(LandType::Plains),
            "island" => Some(LandType::Island),
            "swamp" => Some(LandType::Swamp),
            "mountain" => Some(LandType::Mountain),
            "forest" => Some(LandType::Forest),
            "desert" => Some(LandType::Desert),
            "gate" => Some(LandType::Gate),
            "lair" => Some(LandType::Lair),
            "locus" => Some(LandType::Locus),
            "mine" => Some(LandType::Mine),
            "power-plant" => Some(LandType::PowerPlant),
            "sphere" => Some(LandType::Sphere),
            "tower" => Some(LandType::Tower),
            "urza's" | "urzas" => Some(LandType::Urzas),
            "cave" => Some(LandType::Cave),
            "town" => Some(LandType::Town),
            "planet" => Some(LandType::Planet),
            _ => None,
        }
    }

    #[must_use]
    pub fn lookup_artifact_subtype(raw: &str) -> Option<ArtifactSubtype> {
        match raw.to_ascii_lowercase().as_str() {
            "attraction" => Some(ArtifactSubtype::Attraction),
            "blood" => Some(ArtifactSubtype::Blood),
            "bobblehead" => Some(ArtifactSubtype::Bobblehead),
            "book" => Some(ArtifactSubtype::Book),
            "clue" => Some(ArtifactSubtype::Clue),
            "contraption" => Some(ArtifactSubtype::Contraption),
            "equipment" => Some(ArtifactSubtype::Equipment),
            "food" => Some(ArtifactSubtype::Food),
            "fortification" => Some(ArtifactSubtype::Fortification),
            "gold" => Some(ArtifactSubtype::Gold),
            "incubator" => Some(ArtifactSubtype::Incubator),
            "junk" => Some(ArtifactSubtype::Junk),
            "lander" => Some(ArtifactSubtype::Lander),
            "map" => Some(ArtifactSubtype::Map),
            "mutagen" => Some(ArtifactSubtype::Mutagen),
            "powerstone" => Some(ArtifactSubtype::Powerstone),
            "shard" => Some(ArtifactSubtype::Shard),
            "spacecraft" => Some(ArtifactSubtype::Spacecraft),
            "treasure" => Some(ArtifactSubtype::Treasure),
            "vehicle" => Some(ArtifactSubtype::Vehicle),
            _ => None,
        }
    }

    #[must_use]
    pub fn lookup_enchantment_subtype(raw: &str) -> Option<EnchantmentSubtype> {
        match raw.to_ascii_lowercase().as_str() {
            "background" => Some(EnchantmentSubtype::Background),
            "cartouche" => Some(EnchantmentSubtype::Cartouche),
            "case" => Some(EnchantmentSubtype::Case),
            "class" => Some(EnchantmentSubtype::Class),
            "curse" => Some(EnchantmentSubtype::Curse),
            "role" => Some(EnchantmentSubtype::Role),
            "room" => Some(EnchantmentSubtype::Room),
            "rune" => Some(EnchantmentSubtype::Rune),
            "saga" => Some(EnchantmentSubtype::Saga),
            "shard" => Some(EnchantmentSubtype::Shard),
            "shrine" => Some(EnchantmentSubtype::Shrine),
            _ => None,
        }
    }

    #[must_use]
    pub fn lookup_spell_subtype(raw: &str) -> Option<SpellSubtype> {
        match raw.to_ascii_lowercase().as_str() {
            "adventure" => Some(SpellSubtype::Adventure),
            "arcane" => Some(SpellSubtype::Arcane),
            "lesson" => Some(SpellSubtype::Lesson),
            "trap" => Some(SpellSubtype::Trap),
            "omen" => Some(SpellSubtype::Omen),
            _ => None,
        }
    }

    #[must_use]
    pub fn lookup_planeswalker_subtype(raw: &str) -> Option<PlaneswalkerSubtype> {
        PlaneswalkerSubtype::ALL
            .iter()
            .copied()
            .find(|pw| pw.label().eq_ignore_ascii_case(raw))
    }
}

/// Accumulator for building a single [`ExecutableBody`] with target-capability tracking.
#[derive(Debug, Clone)]
pub(crate) struct BodyAcc {
    pub target: BodyTarget,
    pub target_set: bool,
    pub target_bounds: Option<(u16, u16)>,
    pub up_to_one: bool,
    pub x_target_count: bool,
    pub up_to_x_target_count: bool,
    pub instructions: Vec<Instruction>,
}

impl Default for BodyAcc {
    fn default() -> Self {
        Self::new()
    }
}

impl BodyAcc {
    #[must_use]
    pub fn new() -> Self {
        Self {
            target: BodyTarget::None,
            target_set: false,
            target_bounds: None,
            up_to_one: false,
            x_target_count: false,
            up_to_x_target_count: false,
            instructions: Vec::new(),
        }
    }

    /// Push one instruction into the body.
    pub fn push(&mut self, ins: Instruction) {
        self.instructions.push(ins);
    }

    /// Register a required/optional target on this body.
    pub fn register_target(&mut self, target: BodyTarget, count: TargetCount) -> LowerResult<()> {
        if matches!(target, BodyTarget::None) {
            return Ok(());
        }
        if self.target_set {
            if self.target == target {
                return Ok(());
            }
            return Err(LoweringError::Unsupported("v2:multiple-distinct-targets"));
        }
        self.target = target;
        self.target_set = true;
        match count {
            TargetCount::Exact(1) => {}
            TargetCount::Exact(n) if n > 1 => {
                self.target_bounds = Some((n, n));
            }
            TargetCount::UpTo(1) => {
                if matches!(
                    target,
                    BodyTarget::Object {
                        zone: spellwake_engine::program::TargetZone::OwnGraveyard
                            | spellwake_engine::program::TargetZone::AnyGraveyard { .. }
                            | spellwake_engine::program::TargetZone::Exile {
                                owned_flashback_only: false
                            },
                        ..
                    }
                ) {
                    self.target_bounds = Some((0, 1));
                } else {
                    self.up_to_one = true;
                }
            }
            TargetCount::UpTo(n) if n > 1 => {
                self.target_bounds = Some((0, n));
            }
            TargetCount::VariableX => {
                self.x_target_count = true;
            }
            TargetCount::UpToX => {
                self.up_to_x_target_count = true;
            }
            TargetCount::AnyNumber => {
                self.target_bounds = Some((0, u16::MAX));
            }
            _ => return Err(LoweringError::Unsupported("v2:invalid-target-count")),
        }
        Ok(())
    }

    /// Finalize this accumulator into a validated [`ExecutableBody`].
    pub fn into_body(self) -> LowerResult<ExecutableBody> {
        if self.instructions.is_empty() {
            return Err(LoweringError::Unsupported("v2:empty-body"));
        }
        let mut body = ExecutableBody::new(self.target, self.instructions);
        if let Some((min, max)) = self.target_bounds {
            if matches!(
                body.target,
                BodyTarget::Object {
                    zone: spellwake_engine::program::TargetZone::OwnGraveyard
                        | spellwake_engine::program::TargetZone::AnyGraveyard { .. },
                    ..
                }
            ) {
                body = body.with_graveyard_target_bounds(usize::from(min), usize::from(max));
            } else {
                body = body.with_permanent_target_bounds(min, max);
            }
        }
        if self.up_to_one {
            body = body.with_up_to_one_target();
        }
        if self.x_target_count {
            body = body.with_x_target_count();
        }
        if self.up_to_x_target_count {
            body = body.with_up_to_x_target_count();
        }
        Ok(body)
    }
}
