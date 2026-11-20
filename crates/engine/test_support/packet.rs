//! One input-only packet/seed codec for public tools and cfg(test) actual owners.
//! Preparation constructs values only; it never initializes or mutates an executor.
use super::{creature_types, program};
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeVocabulary, CreatureVocabularyId},
};
use spellwake_engine::{
    combat::CombatRules,
    effects::SeedCard,
    game::{GameContext, GameId},
    mana::FloatingMana,
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    setup::{GameSeed, StartingLife},
};
use std::sync::Arc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
/// serde_json::Value normally accepts repeated object keys. Reject them at every
/// depth before interpreting a selected recipe or any definition/map membership.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON without duplicate object fields")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Unique, E> {
                Ok(Unique(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Unique, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| Unique(Value::Number(number)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(Unique(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!("duplicate object field {key}")));
                    }
                    let Unique(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(Unique(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}
/// Parse a qualification document without accepting duplicate keys at any depth.
/// Individual callers still validate their own document schema and native types.
pub fn unique_json(bytes: &[u8]) -> Result<Value, String> {
    let Unique(value) = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    Ok(value)
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    pub game_id: u128,
    pub context: [String; 5],
    pub creature_vocabulary: String,
    pub name_rules: Vec<spellwake_contracts::names::ObjectNames>,
    pub name_rules_id: String,
    pub participants: Vec<String>,
    pub active: String,
    pub life: BTreeMap<String, RulesInteger>,
    pub combat_rules: String,
    pub initial_boundary: Initial,
    pub floating_mana: Vec<SeedMana>,
    pub cards: Vec<Card>,
    pub initial_battlefield_order: Vec<String>,
    pub initial_world_cohorts: Vec<Vec<String>>,
    pub library_order: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub trusted_precheck_overrides: Vec<Value>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Initial {
    Interaction {},
    TrustedUnitPrecheck {},
    ScheduledPrivatePreparedResourceUnit {
        turn: u64,
        step: String,
    },
    Scheduled {
        turn: u64,
        step: String,
    },
    Opening {
        hand_size: usize,
        entropy_words: Vec<u64>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedMana {
    pub player: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub amount: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub alias: String,
    pub card_id: u64,
    pub owner: String,
    pub controller: Option<String>,
    pub zone: String,
    pub definition: String,
    pub tapped: Option<bool>,
}
pub fn seat(name: &str) -> Result<SeatId, String> {
    match name {
        "A" => Ok(SeatId(0)),
        "B" => Ok(SeatId(1)),
        "C" => Ok(SeatId(2)),
        "D" => Ok(SeatId(3)),
        "E" => Ok(SeatId(4)),
        "F" => Ok(SeatId(5)),
        _ => Err(format!("unknown participant {name}")),
    }
}

/// Validate the explicit current seed relation and its declared content identity.
/// A digest string alone cannot become an engine NameRulesId.
pub fn validated_name_rules(
    groups: Vec<spellwake_contracts::names::ObjectNames>,
    declared_id: &str,
) -> Result<Arc<spellwake_contracts::names::NameRules>, String> {
    let rules =
        spellwake_contracts::names::NameRules::new(groups).map_err(|error| error.to_string())?;
    if rules.id().to_string() != declared_id {
        return Err("name rules identity differs from supplied relation".into());
    }
    Ok(Arc::new(rules))
}

/// A parsed input packet. Definitions remain raw until a selected seed joins them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    raw: Value,
    vocabulary: Arc<CreatureTypeVocabulary>,
}
impl Packet {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let raw = unique_json(bytes)?;
        let object = raw.as_object().ok_or("input packet is not an object")?;
        let allowed = [
            "schema_version",
            "creature_vocabulary",
            "lineage",
            "kind",
            "status",
            "no_expected_outputs",
            "native_or_java_or_checker_execution",
            "source_semantics",
            "identity_contract",
            "definitions",
            "forge_source_pins",
            "operation_lexicon",
            "runtime_join_contract",
            "fixtures",
            "remaining_input_underspecifications",
            "notes",
            "selected_forge_protocol",
            "event_family_contract",
            "initializer_contract",
        ];
        if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(format!("unknown input packet field {key}"));
        }
        if raw["schema_version"] != 10
            || raw["kind"] != "deterministic-input-only-producer-recipes"
            || raw["no_expected_outputs"] != true
        {
            return Err("unsupported input packet identity".into());
        }
        let fixtures = raw["fixtures"].as_array().ok_or("missing fixture list")?;
        let mut ids = BTreeSet::new();
        for fixture in fixtures {
            if !ids.insert(fixture["id"].as_str().ok_or("fixture without ID")?) {
                return Err("duplicate fixture ID".into());
            }
        }
        if !raw["definitions"].is_object() {
            return Err("definitions must be an object".into());
        }
        let vocabulary = Arc::new(creature_types::vocabulary(
            raw.get("creature_vocabulary")
                .ok_or("missing creature_vocabulary")?
                .clone(),
        )?);
        Ok(Self { raw, vocabulary })
    }
    /// Owned definition inputs without selecting or initializing a game fixture.
    pub fn definition_inputs(&self) -> Result<BTreeMap<String, Value>, String> {
        serde_json::from_value(self.raw["definitions"].clone()).map_err(|error| error.to_string())
    }
    /// The original packet's selection metadata, distinct from runtime results.
    pub fn metadata(&self) -> Result<Value, String> {
        let mut metadata = self.raw.clone();
        let object = metadata.as_object_mut().ok_or("packet object lost")?;
        object.remove("fixtures");
        object.remove("definitions");
        Ok(metadata)
    }
    pub fn select(&self, case: &str) -> Result<SelectedInput, String> {
        let fixture = self.raw["fixtures"]
            .as_array()
            .ok_or("missing fixture list")?
            .iter()
            .find(|fixture| fixture["id"] == case)
            .ok_or_else(|| format!("unknown case {case}"))?
            .clone();
        Ok(SelectedInput {
            fixture,
            definitions: self.definition_inputs()?,
            metadata: self.metadata()?,
            vocabulary: Arc::clone(&self.vocabulary),
        })
    }
}
/// Generic selection has no public-root case gate and performs no private operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedInput {
    pub fixture: Value,
    pub definitions: BTreeMap<String, Value>,
    pub metadata: Value,
    pub vocabulary: Arc<CreatureTypeVocabulary>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedMode {
    PublicMain,
    PreparedRoot,
    TrustedUnit,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionPolicy {
    ValidateSelected,
    DeferToOwner,
}
/// Card aliases annotate exact physical CardIds; they never replace incarnation IDs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AliasContext {
    cards: BTreeMap<CardId, String>,
}
impl AliasContext {
    pub fn from_seed(seed: &Seed) -> Result<Self, String> {
        let cards = seed
            .cards
            .iter()
            .map(|card| (CardId(card.card_id), card.alias.clone()))
            .collect::<Vec<_>>();
        Self::from_card_aliases(&cards)
    }
    /// Annotate an actual effect-only seed without introducing game/context data.
    pub fn from_card_aliases(values: &[(CardId, String)]) -> Result<Self, String> {
        let mut aliases = BTreeSet::new();
        let mut cards = BTreeMap::new();
        for (card, alias) in values {
            if !aliases.insert(alias.clone()) || cards.insert(*card, alias.clone()).is_some() {
                return Err("duplicate alias/card ID".into());
            }
        }
        Ok(Self { cards })
    }
    pub fn alias(&self, card: CardId) -> Option<&str> {
        self.cards.get(&card).map(String::as_str)
    }
}
pub struct PreparedSeed {
    pub game: GameId,
    pub context: GameContext,
    pub seed: GameSeed,
    pub initial: Initial,
    pub aliases: AliasContext,
    /// Explicit requested status, not yet applied; only TrustedUnit may contain true.
    pub initial_status: Vec<(CardId, bool)>,
    /// Input-only owner work, not executed by this codec.
    pub trusted_precheck_overrides: Vec<Value>,
}
impl SelectedInput {
    pub fn prepare_seed(
        &self,
        mode: SeedMode,
        policy: DefinitionPolicy,
    ) -> Result<PreparedSeed, String> {
        let raw: Seed =
            serde_json::from_value(self.fixture.get("seed").ok_or("missing seed")?.clone())
                .map_err(|error| error.to_string())?;
        raw.prepare(&self.definitions, mode, policy, &self.vocabulary)
    }
}

impl Seed {
    pub fn prepare(
        &self,
        definitions: &BTreeMap<String, Value>,
        mode: SeedMode,
        policy: DefinitionPolicy,
        vocabulary: &Arc<CreatureTypeVocabulary>,
    ) -> Result<PreparedSeed, String> {
        let raw = self;
        if policy == DefinitionPolicy::DeferToOwner && mode != SeedMode::TrustedUnit {
            return Err(
                "only a trusted unit may defer Program validation to its actual owner".into(),
            );
        }
        let seats = raw
            .participants
            .iter()
            .map(|name| seat(name))
            .collect::<Result<Vec<_>, _>>()?;
        if !(2..=6).contains(&seats.len())
            || seats.iter().copied().collect::<BTreeSet<_>>().len() != seats.len()
            || !raw.participants.contains(&raw.active)
            || raw.life.keys().cloned().collect::<BTreeSet<_>>()
                != raw.participants.iter().cloned().collect()
        {
            return Err("invalid explicit participant/life set".into());
        }
        if raw.combat_rules != "UnrestrictedFreeForAll" {
            return Err("unsupported combat rules".into());
        }
        if mode == SeedMode::PublicMain && !raw.floating_mana.is_empty() {
            return Err("unsupported public root seed policy/resources".into());
        }
        if mode != SeedMode::TrustedUnit && !raw.trusted_precheck_overrides.is_empty() {
            return Err("trusted precheck overrides require a private owner".into());
        }
        match (&raw.initial_boundary, mode) {
            (Initial::Scheduled { .. } | Initial::Opening { .. }, _) => {}
            (Initial::Interaction {}, SeedMode::PreparedRoot | SeedMode::TrustedUnit) => {}
            (
                Initial::TrustedUnitPrecheck {}
                | Initial::ScheduledPrivatePreparedResourceUnit { .. },
                SeedMode::TrustedUnit,
            ) => {}
            _ => return Err("initializer is not admitted by explicit seed mode".into()),
        }
        let aliases = AliasContext::from_seed(raw)?;
        let cards_by_alias = raw
            .cards
            .iter()
            .map(|card| (card.alias.as_str(), CardId(card.card_id)))
            .collect::<BTreeMap<_, _>>();
        let initial_battlefield_order = raw
            .initial_battlefield_order
            .iter()
            .map(|alias| {
                cards_by_alias
                    .get(alias.as_str())
                    .copied()
                    .ok_or_else(|| format!("unknown initial battlefield alias {alias}"))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let initial_world_cohorts = raw
            .initial_world_cohorts
            .iter()
            .map(|cohort| {
                cohort
                    .iter()
                    .map(|alias| {
                        cards_by_alias
                            .get(alias.as_str())
                            .copied()
                            .ok_or_else(|| format!("unknown initial World alias {alias}"))
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut cards = Vec::new();
        let mut libraries = BTreeMap::<String, Vec<String>>::new();
        let mut initial_status = Vec::new();
        for card in &raw.cards {
            if !raw.participants.contains(&card.owner) {
                return Err("foreign card owner".into());
            }
            let owner = seat(&card.owner)?;
            let controller = || -> Result<SeatId, String> {
                let controller = card
                    .controller
                    .as_ref()
                    .ok_or("controlled zone without explicit controller")?;
                if !raw.participants.contains(controller) {
                    return Err("foreign controller".into());
                }
                seat(controller)
            };
            let destination = match card.zone.as_str() {
                "Library" => {
                    libraries
                        .entry(card.owner.clone())
                        .or_default()
                        .push(card.alias.clone());
                    ObjectDestination::LibraryBottom
                }
                "Hand" => ObjectDestination::Hand,
                "Graveyard" => ObjectDestination::Graveyard,
                "Exile" => ObjectDestination::Exile,
                "Command" => ObjectDestination::Command,
                "Battlefield" => {
                    let tapped = card
                        .tapped
                        .ok_or("battlefield without explicit tapped status")?;
                    if tapped && mode != SeedMode::TrustedUnit {
                        return Err("initial tapped status requires a private owner".into());
                    }
                    initial_status.push((CardId(card.card_id), tapped));
                    ObjectDestination::Battlefield {
                        controller: controller()?,
                    }
                }
                "Stack" if mode == SeedMode::TrustedUnit => ObjectDestination::Stack {
                    controller: controller()?,
                },
                _ => return Err(format!("unsupported seed zone {}", card.zone)),
            };
            if card.zone != "Battlefield"
                && (card.tapped.is_some() || (card.zone != "Stack" && card.controller.is_some()))
            {
                return Err("nonbattlefield seed has controller/status".into());
            }
            let definition = definitions
                .get(&card.definition)
                .ok_or_else(|| format!("missing definition {}", card.definition))?;
            let native = definition
                .get("native_program")
                .ok_or("definition lacks native_program")?
                .clone();
            let program = match policy {
                DefinitionPolicy::ValidateSelected => program::decode(native, vocabulary),
                DefinitionPolicy::DeferToOwner => program::decode_for_owner(native, vocabulary),
            }
            .map_err(|error| format!("{}: {error}", card.definition))?;
            cards.push(SeedCard {
                card: CardId(card.card_id),
                owner,
                destination,
                program,
            });
        }
        for player in &raw.participants {
            libraries.entry(player.clone()).or_default();
        }
        if libraries != raw.library_order {
            return Err(
                "physical seed insertion order differs from explicit complete library_order".into(),
            );
        }
        let life = raw
            .participants
            .iter()
            .map(|name| {
                Ok(StartingLife {
                    player: seat(name)?,
                    total: raw.life[name].clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut mana_keys = BTreeSet::new();
        let mana = raw
            .floating_mana
            .iter()
            .map(|value| {
                let player = seat(&value.player)?;
                let mana_type = program::mana(&value.kind)?;
                if !raw.participants.contains(&value.player)
                    || !mana_keys.insert((player, mana_type))
                {
                    return Err("duplicate mana type or foreign mana player".into());
                }
                Ok(FloatingMana {
                    player,
                    mana_type,
                    amount: value.amount.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let name_rules = validated_name_rules(raw.name_rules.clone(), &raw.name_rules_id)?;
        let seed = GameSeed {
            name_rules: Arc::clone(&name_rules),
            creature_vocabulary: Arc::clone(vocabulary),
            seats,
            active: seat(&raw.active)?,
            combat_rules: CombatRules::UnrestrictedFreeForAll,
            cards,
            initial_battlefield_order,
            initial_world_cohorts,
            mana,
            life,
        };
        let [release, rules, catalogue, programs, profile] = raw.context.clone();
        let context = GameContext::new(
            release,
            rules,
            catalogue,
            programs,
            profile,
            CreatureVocabularyId::try_from(raw.creature_vocabulary.clone())
                .map_err(|error| format!("{error:?}"))?,
            name_rules.id(),
        )
        .map_err(|error| format!("{error:?}"))?;
        Ok(PreparedSeed {
            game: GameId(raw.game_id),
            context,
            seed,
            initial: raw.initial_boundary.clone(),
            aliases,
            initial_status,
            trusted_precheck_overrides: raw.trusted_precheck_overrides.clone(),
        })
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;

    #[test]
    fn effect_only_aliases_keep_exact_card_ids_and_reject_ambiguous_annotations() {
        let first = (CardId(41), "observer".to_owned());
        let second = (CardId(3), "entrant".to_owned());
        let aliases = AliasContext::from_card_aliases(&[first.clone(), second]).unwrap();
        assert_eq!(aliases.alias(CardId(41)), Some("observer"));
        assert_eq!(aliases.alias(CardId(3)), Some("entrant"));
        assert_eq!(aliases.alias(CardId(9)), None);
        assert!(
            AliasContext::from_card_aliases(&[first.clone(), (CardId(41), "other".to_owned()),])
                .is_err()
        );
        assert!(
            AliasContext::from_card_aliases(&[first, (CardId(9), "observer".to_owned()),]).is_err()
        );
    }

    #[test]
    fn independent_document_schemas_share_duplicate_key_rejection_before_interpretation() {
        let actual = unique_json(br#"{"units":[{"id":"M01","value":"1"}]}"#).unwrap();
        assert_eq!(actual["units"][0]["value"], "1");
        assert!(unique_json(br#"{"units":[{"id":"M01","id":"M02"}]}"#).is_err());
        assert!(unique_json(br#"{"units":[],"units":[]}"#).is_err());
    }
    fn strict_packet() -> Value {
        let vocabulary =
            super::super::creature_types::synthetic_vocabulary_for_rules("CR-synthetic");
        serde_json::json!({"schema_version":10,"kind":"deterministic-input-only-producer-recipes","no_expected_outputs":true,"creature_vocabulary":super::super::projection::vocabulary(&vocabulary),"definitions":{},"fixtures":[{"id":"synthetic","seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"game_id":1,"context":["synthetic","CR-synthetic","synthetic","synthetic","synthetic"],"creature_vocabulary":vocabulary.id().to_string(),"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"Interaction"},"floating_mana":[],"cards":[],"initial_world_cohorts":[],"initial_battlefield_order":[],"library_order":{"A":[],"B":[]}}}]})
    }
    #[test]
    fn schema10_requires_its_explicit_vocabulary_and_seed_context_identity() {
        let value = strict_packet();
        let packet = Packet::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            packet
                .select("synthetic")
                .unwrap()
                .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
                .is_ok()
        );
        for version in 1..=9 {
            let mut old = value.clone();
            old["schema_version"] = version.into();
            assert!(Packet::parse(&serde_json::to_vec(&old).unwrap()).is_err());
        }
        let mut absent = value.clone();
        absent
            .as_object_mut()
            .unwrap()
            .remove("creature_vocabulary");
        assert!(Packet::parse(&serde_json::to_vec(&absent).unwrap()).is_err());
        let mut absent = value;
        absent["fixtures"][0]["seed"]
            .as_object_mut()
            .unwrap()
            .remove("creature_vocabulary");
        let packet = Packet::parse(&serde_json::to_vec(&absent).unwrap()).unwrap();
        assert!(
            packet
                .select("synthetic")
                .unwrap()
                .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
                .is_err()
        );
    }
    #[test]
    fn current_seed_requires_explicit_world_cohorts_and_resolves_only_known_aliases() {
        let value = strict_packet();
        for field in [
            None,
            Some(Value::Null),
            Some(serde_json::json!(["not-a-cohort"])),
            Some(serde_json::json!([["missing-card"]])),
        ] {
            let mut raw = value.clone();
            if let Some(field) = field {
                raw["fixtures"][0]["seed"]["initial_world_cohorts"] = field;
            } else {
                raw["fixtures"][0]["seed"]
                    .as_object_mut()
                    .unwrap()
                    .remove("initial_world_cohorts");
            }
            let packet = Packet::parse(&serde_json::to_vec(&raw).unwrap()).unwrap();
            assert!(
                packet
                    .select("synthetic")
                    .unwrap()
                    .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
                    .is_err()
            );
        }
    }
    #[test]
    fn syntactically_valid_foreign_context_reaches_actual_initializer_invalid_context() {
        for foreign_rules in [false, true] {
            let mut value = strict_packet();
            if foreign_rules {
                value["fixtures"][0]["seed"]["context"][1] = serde_json::json!("CR-other");
            } else {
                value["fixtures"][0]["seed"]["creature_vocabulary"] =
                    serde_json::json!("f".repeat(64));
            }
            let packet = Packet::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
            let prepared = packet
                .select("synthetic")
                .unwrap()
                .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
                .unwrap();
            assert_eq!(
                spellwake_engine::game::GameState::initialize(
                    prepared.game,
                    prepared.context,
                    prepared.seed
                ),
                Err(spellwake_engine::game::GameError::InvalidContext)
            );
        }
    }
    #[test]
    fn name_relation_is_required_and_its_identity_is_derived_from_validated_content() {
        let source = strict_packet();
        for field in ["name_rules", "name_rules_id"] {
            let mut missing = source.clone();
            missing["fixtures"][0]["seed"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            let packet = Packet::parse(&serde_json::to_vec(&missing).unwrap()).unwrap();
            assert!(
                packet
                    .select("synthetic")
                    .unwrap()
                    .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
                    .is_err()
            );
        }
        for groups in [
            serde_json::json!([["A"]]),
            serde_json::json!([["A", "B"], ["B", "C"]]),
            serde_json::json!([["A", "B"]]),
        ] {
            let mut invalid = source.clone();
            invalid["fixtures"][0]["seed"]["name_rules"] = groups;
            let packet = Packet::parse(&serde_json::to_vec(&invalid).unwrap()).unwrap();
            assert!(
                packet
                    .select("synthetic")
                    .unwrap()
                    .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
                    .is_err()
            );
        }
        let groups: Vec<spellwake_contracts::names::ObjectNames> =
            serde_json::from_value(serde_json::json!([["A", "B"]])).unwrap();
        let relation = spellwake_contracts::names::NameRules::new(groups.clone()).unwrap();
        let mut valid = source;
        valid["fixtures"][0]["seed"]["name_rules"] = serde_json::json!(groups);
        valid["fixtures"][0]["seed"]["name_rules_id"] = serde_json::json!(relation.id());
        let packet = Packet::parse(&serde_json::to_vec(&valid).unwrap()).unwrap();
        let prepared = packet
            .select("synthetic")
            .unwrap()
            .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
            .unwrap();
        assert_eq!(prepared.context.name_rules_id(), relation.id());
        assert_eq!(*prepared.seed.name_rules, relation);
    }
}
