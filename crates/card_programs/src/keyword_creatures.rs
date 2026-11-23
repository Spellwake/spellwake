//! Closed compilation of complete keyword-only faces from the dated catalogue.
use super::*;
mod advanced_combat;
mod army_grants;
mod artifact_activations;
mod ascend;
pub(crate) mod attachments;
mod attack_rules;
mod attacking_tokens;
mod avacyn_restored;
mod banding;
mod basic_land_search;
mod basic_lands;
mod battles;
mod chosen_type_mana;
mod combat_triggers;
mod commander_mana;
mod conditional_mana;
mod counted_mana;
mod counter_keywords;
pub(crate) mod crew_saddle;
mod cycling;
mod daybound;
mod defining_stats;
mod destiny_spinner;
mod devotion;
mod disturb;
mod drakuseth;
mod enchantment_watchers;
mod escape;
mod evoke;
mod exile_hand;
pub(crate) mod face_down;
mod flash_permissions;
mod granted_mana;
mod graveyard_return;
mod growth;
mod hybrid_filter;
mod innistrad;
mod library_access;
mod lieutenant;
mod limited_activations;
mod linked_exile;
mod mana_permanents;
mod meld;
mod monarch;
mod mutate;
mod ninja;
mod opponents_mana;
pub(crate) mod oracle_lowering;
mod ordinary_text;
mod ordinary_triggers;
mod paid_color_mana;
mod planeswalkers;
mod power_mana;
mod power_up;
mod reductions;
mod reveal_lands;
mod sagas;
mod soulbond;
mod source_boost;
mod speed;
mod spell_token_watchers;
mod spells;
mod split_spells;
mod static_grants;
mod station;
mod thriving_mana;
mod thryx;
mod token_activations;
mod transfers;
mod transforming;
mod upkeep;
mod urza_mana;
mod variable_mana;
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId},
    program::CastReduction,
};

/// Latest source-guarded keyword compiler, including mechanisms introduced after registry 14.
pub const KEYWORD_GAMEPLAY_V4_PROFILE: &str = "keyword-gameplay-v4";
/// Stable source compiler for registry revision 16.
pub const KEYWORD_GAMEPLAY_V5_PROFILE: &str = "keyword-gameplay-v5";
/// Stable source compiler for registry revision 17.
pub const KEYWORD_GAMEPLAY_V6_PROFILE: &str = "keyword-gameplay-v6";
/// Rolling source compiler for the newest supported abilities.
pub const CURRENT_KEYWORD_PROFILE: &str = "keyword-gameplay-current";
/// Combined profile for source-guarded cards with executable keyword mechanics.
pub const KEYWORD_GAMEPLAY_V3_PROFILE: &str = "keyword-gameplay-v3";
/// Second keyword gameplay profile.
pub const KEYWORD_GAMEPLAY_PROFILE: &str = "keyword-gameplay-v2";
/// Registry revision for the keyword gameplay profile.
pub const KEYWORD_GAMEPLAY_REGISTRY_VERSION: u32 = 13;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Profile {
    CreatureV1,
    GameplayV2,
    GameplayV3,
    GameplayV4,
    GameplayV5,
    GameplayV6,
    Current,
}

use spellwake_contracts::names::{EnglishCardName, ObjectNames};
use spellwake_engine::{
    characteristics::KeywordAbility,
    program::{CardType, Supertype, TriggeredAbility, TriggeredAbilityId},
};

/// Independent registry profile for ordinary creatures with only static keywords.
pub const KEYWORD_CREATURE_PROFILE: &str = "keyword-creatures-v1";
/// Registry revision for the keyword-only creature compiler.
pub const KEYWORD_CREATURE_REGISTRY_VERSION: u32 = 12;

// SHA-256 of compact serde JSON for the saved ordered InventoryCard array. Includes
// every printing, identity, face, legality and required flag, including exclusions.
// The raw-source test independently verifies every bound card’s colors and
// nonappearance of color indicators, which the reduced inventory does not carry.
const MATERIALIZED_CARDS_SHA256: &str =
    "c5fc3cf09c408e0ca5085daa64a23f9790f96bf1a85ebd9fb481d6ff3fefa305";

impl Registry {
    /// Compile the fixed-cost, fixed-stat keyword-only creature faces and report an
    /// admission result for every required Oracle identity in the pinned snapshot.
    /// Source and definition validation are separate from gameplay qualification.
    ///
    /// # Errors
    /// Rejects a foreign snapshot, altered materialized inventory, foreign vocabulary,
    /// invalid native definition, invalid English name or semantic signature failure.
    pub fn from_keyword_creature_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_inventory(inventory, vocabulary, Profile::CreatureV1)
    }

    /// Materialize the second keyword profile, including activated, triggered,
    /// attachment and casting-cost definitions supported by this compiler.
    ///
    /// # Errors
    /// Returns the same source, vocabulary and definition validation errors as
    /// `from_keyword_creature_inventory`.
    pub fn from_keyword_gameplay_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_inventory(inventory, vocabulary, Profile::GameplayV2)
    }

    /// Compile the third source-guarded keyword profile, retaining the earlier profiles.
    /// # Errors
    /// Returns an error for malformed definitions, mismatched input snapshots or vocabularies.
    pub fn from_extended_keyword_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_inventory(inventory, vocabulary, Profile::GameplayV3)
    }
    /// Build the current gameplay registry from the exact saved inventory and matching vocabulary.
    ///
    /// # Errors
    /// Returns a source or vocabulary error on mismatch, or an error for an invalid compiled definition.
    pub fn from_current_keyword_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_inventory(inventory, vocabulary, Profile::Current)
    }
    /// Compile the fourth profile, retaining its original registry keys and source admissions.
    /// # Errors
    /// Rejects foreign catalogue data, vocabulary or invalid native definitions.
    pub fn from_fourth_keyword_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_inventory(inventory, vocabulary, Profile::GameplayV4)
    }
    /// Compile the fifth source-guarded keyword profile.
    /// # Errors
    /// Rejects foreign catalogue data, vocabulary or invalid native definitions.
    pub fn from_fifth_keyword_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_inventory(inventory, vocabulary, Profile::GameplayV5)
    }
    /// Compile the sixth source-guarded keyword profile.
    /// # Errors
    /// Rejects foreign catalogue data, vocabulary or invalid native definitions.
    pub fn from_sixth_keyword_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_inventory(inventory, vocabulary, Profile::GameplayV6)
    }
    fn compile_keyword_inventory(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
        profile: Profile,
    ) -> Result<Self, RegistryError> {
        Self::compile_keyword_selection(inventory, vocabulary, profile, None)
    }

    fn compile_keyword_selection(
        inventory: &Inventory,
        vocabulary: &CreatureTypeVocabulary,
        profile: Profile,
        selected: Option<&BTreeSet<String>>,
    ) -> Result<Self, RegistryError> {
        if !saved_inventory_matches(inventory) || inventory.required_records != 32_155 {
            return Err(RegistryError::KeywordCreatureSourceMismatch);
        }
        if vocabulary.id().to_string() != TEACHING_VOCABULARY_ID {
            return Err(RegistryError::KeywordCreatureVocabularyMismatch);
        }
        let mut digest = Sha256::new();
        serde_json::to_writer(&mut digest, &inventory.cards).map_err(RegistryError::Encoding)?;
        if format!("{:x}", digest.finalize()) != MATERIALIZED_CARDS_SHA256 {
            return Err(RegistryError::KeywordCreatureSourceMismatch);
        }

        // Bind source loyalty separately without changing legacy materialization.
        let loyalty_source_verified = if profile == Profile::Current {
            inventory
                .source_loyalty_sha256()
                .map_err(RegistryError::Encoding)?
                == planeswalkers::LOYALTY_SOURCE_SHA256
        } else {
            false
        };
        if profile == Profile::Current && !loyalty_source_verified {
            return Err(RegistryError::KeywordCreatureSourceMismatch);
        }
        let mut nonland_names = None;
        let mut bindings = BTreeMap::new();
        let mut invalid_definitions = Vec::new();
        let mut cards =
            Vec::with_capacity(selected.map_or(inventory.required_records, BTreeSet::len));
        for card in inventory
            .cards
            .iter()
            .filter(|card| card.required_by_base_formats)
            .filter(|card| {
                selected
                    .is_none_or(|ids| card.oracle_id.as_ref().is_some_and(|id| ids.contains(id)))
            })
        {
            let transforming = (profile == Profile::Current && card.layout == "transform")
                .then(|| {
                    let indicator = card
                        .oracle_id
                        .as_deref()
                        .and_then(|id| inventory.back_color_indicator(id));
                    daybound::compile(card, vocabulary, indicator)
                        .or_else(|| battles::compile(card, inventory, vocabulary))
                        .or_else(|| disturb::compile(card, vocabulary, indicator))
                        .or_else(|| transforming::compile(card, vocabulary, indicator))
                        .or_else(|| {
                            planeswalkers::compile_transforming(
                                card,
                                inventory,
                                vocabulary,
                                indicator,
                                loyalty_source_verified,
                            )
                        })
                        .or_else(|| {
                            oracle_lowering::compile_transforming(card, vocabulary, indicator)
                        })
                })
                .flatten();
            // The existing compilers read the first side as the base price. Preserve all
            // printed alternatives on the resulting program for announcement and devotion.
            let hybrid_face = (profile == Profile::Current && card.faces.len() == 1)
                .then(|| split_hybrid_cost(card.faces[0].mana_cost.as_deref()?))
                .flatten();
            let normalized = hybrid_face.as_ref().map(|(cost, _)| {
                let mut normal = card.clone();
                normal.faces[0].mana_cost = Some(cost.clone());
                normal
            });
            let compiler_input = normalized.as_ref().unwrap_or(card);
            let walker = (profile == Profile::Current)
                .then(|| {
                    planeswalkers::compile(
                        compiler_input,
                        inventory,
                        vocabulary,
                        loyalty_source_verified,
                    )
                    .or_else(|| {
                        oracle_lowering::compile_planeswalker(
                            compiler_input,
                            inventory,
                            vocabulary,
                            loyalty_source_verified,
                        )
                    })
                })
                .flatten();
            let meld = (profile == Profile::Current && card.layout == "meld")
                .then(|| meld::compile(compiler_input, inventory, vocabulary))
                .flatten();
            let name_restriction = (profile == Profile::Current && card.name == "Nevermore")
                .then(|| {
                    innistrad::compile_name_restriction(
                        compiler_input,
                        inventory,
                        &mut nonland_names,
                    )
                })
                .flatten();
            let definition = walker
                .or(transforming)
                .or(meld)
                .or(name_restriction)
                .map_or_else(|| compile(compiler_input, vocabulary, profile), Ok)
                .map(|mut program| {
                    if let Some((_, symbols)) = hybrid_face {
                        program.hybrid_symbols = symbols;
                    }
                    program
                });
            let binding = match definition {
                Err(reason) => BindingStatus::Blocked { reason },
                Ok(mut program) => {
                    let id = card
                        .oracle_id
                        .clone()
                        .and_then(|id| OracleId::try_from(id).ok())
                        .ok_or(RegistryError::KeywordCreatureSourceMismatch)?;
                    if program.split.is_none() {
                        program.names = ObjectNames::single(
                            EnglishCardName::new(
                                if program.reverse_face().is_some() || program.adventure.is_some() {
                                    card.faces[0].name.clone()
                                } else {
                                    card.name.clone()
                                },
                            )
                            .map_err(RegistryError::Name)?,
                        );
                    }
                    if let Err(reason) = program.validate(vocabulary) {
                        invalid_definitions.push((id, reason));
                        continue;
                    }
                    let semantic_sha256 = signature(card, &id)?;
                    let (key, scope) = match profile {
                        Profile::CreatureV1 => (
                            ProgramKey::KeywordCreatureV1,
                            Scope::KeywordCreatureOrdinaryCardV1,
                        ),
                        Profile::GameplayV4 => (
                            ProgramKey::KeywordGameplayCurrent,
                            Scope::KeywordGameplayCurrent,
                        ),
                        Profile::GameplayV6 => {
                            (ProgramKey::KeywordGameplayV6, Scope::KeywordGameplayV6)
                        }
                        Profile::Current => {
                            (ProgramKey::KeywordGameplayV7, Scope::KeywordGameplayV7)
                        }
                        Profile::GameplayV5 => {
                            (ProgramKey::KeywordGameplayV5, Scope::KeywordGameplayV5)
                        }
                        Profile::GameplayV3 => {
                            (ProgramKey::KeywordGameplayV3, Scope::KeywordGameplayV3)
                        }
                        Profile::GameplayV2 => {
                            (ProgramKey::KeywordGameplayV2, Scope::KeywordGameplayV2)
                        }
                    };
                    bindings.insert(
                        id,
                        BoundProgram {
                            key,
                            scope,
                            instruction: program,
                        },
                    );
                    BindingStatus::Partial {
                        program: key,
                        scope,
                        semantic_sha256,
                    }
                }
            };
            cards.push(CardCoverage {
                name: card.name.clone(),
                oracle_id: card.oracle_id.clone(),
                required: true,
                binding,
                behavior_evidence: "not-evaluated-by-registry",
            });
        }
        if invalid_definitions.len() == 1
            && let Some((oracle_id, reason)) = invalid_definitions.pop()
        {
            return Err(RegistryError::InvalidKeywordCreatureDefinition { oracle_id, reason });
        }
        if !invalid_definitions.is_empty() {
            return Err(RegistryError::InvalidKeywordCreatureDefinitions(
                invalid_definitions,
            ));
        }
        let coverage = Coverage {
            source_sha256: inventory.compressed_sha256.clone(),
            source_records: inventory.cards.len(),
            required_records: cards.len(),
            required_partial_bindings: bindings.len(),
            required_without_binding: cards.len() - bindings.len(),
            fully_qualified_cards: 0,
            behavior_evidence: "not-evaluated-by-registry",
            registry_version: match profile {
                Profile::CreatureV1 => KEYWORD_CREATURE_REGISTRY_VERSION,
                Profile::GameplayV2 => KEYWORD_GAMEPLAY_REGISTRY_VERSION,
                Profile::GameplayV3 => 14,
                Profile::GameplayV4 => 15,
                Profile::GameplayV5 => 16,
                Profile::GameplayV6 => 17,
                Profile::Current => 18,
            },
            creature_vocabulary_id: vocabulary.id(),
            cards,
        };
        Ok(Self { bindings, coverage })
    }
}

fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Result<Program, &'static str> {
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some(rules) = face.oracle_text.as_deref().and_then(without_reminder)
        && let Some((ordinary, miracle)) = rules.trim().rsplit_once('\n')
        && let Some(price) = miracle.trim().strip_prefix("Miracle ")
    {
        let (mana, x_symbols) =
            avacyn_restored::cost_with_x(price).ok_or("unsupported-miracle-cost")?;
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(ordinary.trim().to_owned());
        let mut program = compile(&reduced, vocabulary, profile)?;
        if program.miracle.is_some() {
            return Err("multiple-miracle-abilities");
        }
        program.miracle = Some(spellwake_engine::program::MiracleCost { mana, x_symbols });
        return Ok(program);
    }
    if profile == Profile::Current
        && card.faces.iter().any(|face| {
            face.oracle_text
                .as_deref()
                .is_some_and(|text| text.lines().any(|line| line.starts_with("Mutate ")))
        })
    {
        return mutate::compile(card, vocabulary).ok_or("requires-other-mutate-rules");
    }
    if profile == Profile::Current && card.layout == "split" {
        return split_spells::compile(card, vocabulary, profile);
    }
    if profile == Profile::Current
        && card.layout == "adventure"
        && let [front, back] = card.faces.as_slice()
    {
        let mut face_card = card.clone();
        face_card.layout = "normal".to_owned();
        face_card.name = front.name.clone();
        face_card.faces = vec![front.clone()];
        let mut program = compile(&face_card, vocabulary, profile)?;
        if program.permanent().is_none() {
            return Err("outside-adventure-main-face");
        }
        let subtype = back
            .type_line
            .as_deref()
            .and_then(|line| line.strip_suffix(" — Adventure"))
            .filter(|kind| matches!(*kind, "Instant" | "Sorcery"))
            .ok_or("outside-adventure-spell-type")?;
        face_card.name = back.name.clone();
        face_card.faces = vec![back.clone()];
        face_card.faces[0].type_line = Some(subtype.to_owned());
        let mut adventure = compile(&face_card, vocabulary, profile)?;
        adventure.names = ObjectNames::single(
            EnglishCardName::new(back.name.clone()).map_err(|_| "outside-adventure-name")?,
        );
        adventure
            .spell_subtypes
            .insert(spellwake_engine::program::SpellSubtype::Adventure);
        program.adventure = Some(Box::new(adventure));
        return Ok(program);
    }

    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some(text) = face.oracle_text.as_deref().and_then(without_reminder)
        && let Some((header, rest)) = text.split_once('\n')
        && header.trim() == "Kicker {X}"
        && let Some((remaining, trigger)) = rest.rsplit_once('\n')
        && let Some((subject, token)) = trigger
            .strip_prefix("When ")
            .and_then(|s| s.strip_suffix(" tokens."))
            .and_then(|s| s.split_once(" enters, if it was kicked, create X "))
        && (subject == card.name
            || card
                .name
                .strip_prefix(subject)
                .is_some_and(|rest| rest.starts_with(" the ") || rest.starts_with(", ")))
        && let Some(spellwake_engine::program::Instruction::CreateTokens { mut creations }) =
            ordinary_text::create_tokens(&format!("a {token} token"), vocabulary)
        && let [creation] = creations.as_mut_slice()
    {
        use spellwake_engine::program::{
            AdditionalCastCost, BodyTarget, ExecutableBody, Instruction, TriggerGrouping,
        };
        creation.amount = spellwake_engine::program::ValueExpr::event(
            spellwake_engine::program::EventValueKind::EnteredKickerX,
        );
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(remaining.to_owned());
        let mut program = compile(&reduced, vocabulary, profile)?;
        if program.permanent().is_some() && program.additional_cast_costs.is_empty() {
            program
                .additional_cast_costs
                .push(AdditionalCastCost::KickerX(
                    spellwake_engine::mana::ManaCost::free(),
                ));
            let id = program
                .triggered
                .iter()
                .map(|t| t.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("too-many-triggered-abilities")?;
            program.triggered.push(TriggeredAbility {
                id: TriggeredAbilityId(id),
                condition: spellwake_engine::program::TriggerSpec::new(
                    spellwake_engine::program::TriggerEvent::Enters {
                        subject: spellwake_engine::program::ObjectFilter::self_object(),
                        from_zone: spellwake_engine::program::TriggerFromZone::Any,
                        cast_origin: Some(spellwake_engine::program::CastEntryKind::Kicked),
                        attached_to_creature: false,
                    },
                ),
                grouping: TriggerGrouping::EachOccurrence,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::CreateTokens { creations }],
                ),
            });
            return Ok(program);
        }
    }
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some(text) = face.oracle_text.as_deref().and_then(without_reminder)
        && let Some((devour, other)) = text.split_once('\n')
        && let Some(n) = devour
            .trim()
            .strip_prefix("Devour ")
            .and_then(|n| n.parse::<u8>().ok())
        && other.trim()
            == "Each other creature you control enters with an additional X +1/+1 counters on it, where X is the number of +1/+1 counters on this creature."
    {
        let mut program = ordinary_creature(card, vocabulary, profile, Some(""))?;
        program
            .entry_counter_abilities
            .push(spellwake_engine::program::EntryCounterAbility::Devour(n));
        program.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::Modifier { id: spellwake_engine::characteristics::StaticAbilityId(0), rule: spellwake_engine::characteristics::StaticModifierRule::OtherCreaturesEnterWithSourceCounters });
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = soulbond::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = ascend::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = library_access::impulse(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = library_access::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = banding::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = drakuseth::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = destiny_spinner::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = thryx::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = defining_stats::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = defining_stats::count_based_modifier(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = defining_stats::counter_threshold_keywords(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = planeswalkers::loyalty_support(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = attack_rules::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = variable_mana::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = transfigure(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = speed::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = basic_land_search::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if let Some(program) = graveyard_return::compile(card, vocabulary, profile) {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = devotion::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = lieutenant::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = sagas::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = linked_exile::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = reductions::battlefield(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = chosen_type_mana::choose_and_gain_type(card, vocabulary)
            .or_else(|| chosen_type_mana::chosen_type_across_zones(card, vocabulary))
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = counter_keywords::adapt(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = enchantment_watchers::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = spell_token_watchers::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if let Some(program) = commander_mana::compile(card, profile) {
        return Ok(program);
    }
    if let Some(program) = thriving_mana::compile(card, profile) {
        return Ok(program);
    }
    if let Some(program) = reveal_lands::compile(card, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::compile(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::fall_from_favor(card, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::entourage(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::behemoth(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = granted_mana::lantern(card, profile) {
        return Ok(program);
    }
    if let Some(program) = granted_mana::rishkar(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = opponents_mana::compile(card, profile) {
        return Ok(program);
    }
    if let Some(program) = conditional_mana::shrine(card, profile) {
        return Ok(program);
    }
    if let Some(program) = conditional_mana::eldrazi_temple(card, profile) {
        return Ok(program);
    }
    if let Some(program) = conditional_mana::mage_ring(card, profile) {
        return Ok(program);
    }
    if let Some(program) = conditional_mana::tomb_of_spirit_dragon(card, profile) {
        return Ok(program);
    }
    if let Some(program) = conditional_mana::beacon(card, profile) {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = chosen_type_mana::colored_reduction(card, vocabulary)
    {
        return Ok(program);
    }
    if let Some(program) = chosen_type_mana::compile(card, profile) {
        return Ok(program);
    }
    if let Some(program) = chosen_type_mana::jeweled_lotus(card, profile) {
        return Ok(program);
    }
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some((line, rest)) = face
            .oracle_text
            .as_deref()
            .and_then(|text| text.split_once('\n'))
        && let Some(subject) =
            line.strip_suffix(" enters under the control of an opponent of your choice.")
        && (subject == face.name
            || face
                .name
                .split_once(',')
                .is_some_and(|(short, _)| short == subject))
    {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(rest.to_owned());
        let mut program = compile(&reduced, vocabulary, profile)?;
        if let spellwake_engine::program::ProgramKind::Permanent(def) = &mut program.kind
            && def.entry_tap.is_none()
        {
            def.entry_tap = Some(spellwake_engine::land::EntryTap::ChosenOpponentControls);
            return Ok(program);
        }
    }
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some(text) = face.oracle_text.as_deref()
        && let Some((first, remainder)) = text.split_once('\n')
        && let Some((price, objects)) = first
            .strip_prefix("You may pay ")
            .and_then(|s| s.strip_suffix(" rather than pay this spell's mana cost."))
            .and_then(|s| s.split_once(" and tap "))
        && let Some((count, quality)) = (match objects {
            "an untapped artifact you control" => {
                Some((1, spellwake_engine::program::CastTapQuality::Artifact))
            }
            "four untapped creatures you control with flying" => {
                Some((4, spellwake_engine::program::CastTapQuality::FlyingCreature))
            }
            _ => None,
        })
        && let Some(mana) = fixed_cost(price)
    {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(remainder.to_owned());
        let mut program = compile(&reduced, vocabulary, profile)?;
        if program.conditional_cost.is_none() && !program.has_type(CardType::Land) {
            program.conditional_cost = Some(
                spellwake_engine::program::ConditionalCastCost::TapPermanents {
                    mana,
                    count,
                    quality,
                },
            );
            return Ok(program);
        }
    }
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some(text) = face.oracle_text.as_deref()
        && let Some(remainder) =
            text.strip_prefix("As an additional cost to cast this spell, sacrifice a creature.\n")
    {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(remainder.to_owned());
        let mut program = compile(&reduced, vocabulary, profile)?;
        if !program.sacrifice_creature_cast_cost && !program.has_type(CardType::Land) {
            program.sacrifice_creature_cast_cost = true;
            return Ok(program);
        }
    }
    if profile == Profile::Current && let [face] = card.faces.as_slice()
        && face.type_line.as_deref() == Some("Artifact")
        && let Some(text) = face.oracle_text.as_deref()
        && text.lines().eq(["Indestructible","Once each turn, you may pay {0} rather than pay the mana cost for a colorless spell you cast from your hand."])
        && let Some(cost) = face.mana_cost.as_deref().and_then(fixed_cost) {
        let mut program=Program::artifact(Some(cost),vec![]);
        program.printed_keywords.insert(KeywordAbility::Indestructible);
        program.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::CostModifier { id: spellwake_engine::characteristics::StaticAbilityId(0), modifier: spellwake_engine::characteristics::SpellCostModifier::FreeColorlessHandSpellOnce });
        return Ok(program);
    }
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some(text) = face.oracle_text.as_deref()
    {
        let permission = "Once during each of your turns, you may cast an enchantment spell by paying life equal to its mana value rather than paying its mana cost.";
        if text.lines().any(|line| line == permission) {
            let mut reduced = card.clone();
            reduced.faces[0].oracle_text = Some(
                text.lines()
                    .filter(|line| *line != permission)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            let mut program = compile(&reduced, vocabulary, profile)?;
            if program.permanent().is_some() && program.continuous.is_none() {
                program.continuous=Some(spellwake_engine::characteristics::StaticContinuousAbility::CostModifier { id: spellwake_engine::characteristics::StaticAbilityId(0), modifier: spellwake_engine::characteristics::SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce });
                return Ok(program);
            }
        }
    }
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && let Some(text) = face.oracle_text.as_deref()
        && let Some((line, rest)) = text.split_once('\n')
        && let Some(counters) = line
            .strip_prefix("This artifact enters with ")
            .or_else(|| line.strip_prefix("This land enters with "))
        && let Some(ability) = {
            use spellwake_engine::program::EntryCounterAbility;
            let counter = |word: &str| {
                counters
                    .strip_suffix(&format!(" {word} counters on it."))
                    .or_else(|| counters.strip_suffix(&format!(" {word} counter on it.")))
                    .and_then(ordinary_text::amount)
            };
            counter("charge")
                .map(|count| EntryCounterAbility::FixedCharge(count.into()))
                .or_else(|| {
                    counter("loyalty").map(|count| EntryCounterAbility::FixedLoyalty(count.into()))
                })
        }
    {
        let mut reduced = card.clone();
        reduced.faces[0].oracle_text = Some(rest.to_owned());
        let mut program = compile(&reduced, vocabulary, profile)?;
        if program.permanent().is_some() {
            program.entry_counter_abilities.push(ability);
            return Ok(program);
        }
    }
    if profile == Profile::Current
        && let [face] = card.faces.as_slice()
        && face.type_line.as_deref() == Some("Artifact")
        && let Some(rules) = face.oracle_text.as_deref().and_then(without_reminder)
    {
        let mut paragraphs = rules.lines().map(str::trim).filter(|line| !line.is_empty());
        if let Some(price) = paragraphs
            .next()
            .and_then(|line| line.strip_prefix("Multikicker "))
            .and_then(fixed_cost)
            && paragraphs.eq([
                "This artifact enters with a charge counter on it for each time it was kicked.",
                "{T}: Add {C} for each charge counter on this artifact.",
            ])
            && let Some(cost) = face.mana_cost.as_deref().and_then(fixed_cost)
        {
            let mut program = Program::artifact(Some(cost), vec![]);
            program.mana.push(spellwake_engine::mana::ManaAbility {
                id: spellwake_engine::mana::ManaAbilityId(0),
                output: spellwake_engine::mana::ManaOutput::CounterBased(
                    spellwake_engine::mana::CounterManaYield::SourceChargeCounters,
                ),
            });
            program.additional_cast_costs.push(
                spellwake_engine::program::AdditionalCastCost::Multikicker(price),
            );
            program
                .entry_counter_abilities
                .push(spellwake_engine::program::EntryCounterAbility::ChargePerKicker(0));
            return Ok(program);
        }
    }
    if profile==Profile::Current && let [face]=card.faces.as_slice() && let Some(text)=face.oracle_text.as_deref()
        && let Some(rest)=text.strip_prefix("This artifact enters with a number of suspect counters on it equal to the greatest number of creatures a player controls.\n") {
        let mut reduced=card.clone();reduced.faces[0].oracle_text=Some(rest.to_owned());
        let mut program=compile(&reduced,vocabulary,profile)?;
        if program.has_type(CardType::Artifact) {
            program.entry_counter_abilities.push(spellwake_engine::program::EntryCounterAbility::SuspectForGreatestCreatureCount);
            return Ok(program);
        }
    }
    if profile == Profile::Current && let [face] = card.faces.as_slice() && let Some(text)=face.oracle_text.as_deref()
        && let Some(remaining) = text.strip_prefix(&format!("If {} is your commander, choose a color before the game begins. {} is the chosen color.\n",face.name,face.name)) {
        use spellwake_engine::characteristics::{StaticContinuousAbility,StaticAbilityId,DefiningParts,DefiningPart};
        let mut program=ordinary_creature(card,vocabulary,profile,Some(remaining))?;
        if program.continuous.is_none() {
            program.continuous=Some(StaticContinuousAbility::CharacteristicDefining { id:StaticAbilityId(0), parts:DefiningParts(vec![DefiningPart::CommanderChosenColor(None)])});
            return Ok(program);
        }
    }
    if let Some(program) = mana_permanents::bonders_ornament(card, profile) {
        return Ok(program);
    }
    if let Some(program) = mana_permanents::miraris_wake(card, profile) {
        return Ok(program);
    }
    if let Some(program) = mana_permanents::forsaken_monument(card, profile) {
        return Ok(program);
    }
    if let Some(program) = mana_permanents::spectral_searchlight(card, profile) {
        return Ok(program);
    }
    if let Some(program) = mana_permanents::great_henge(card, profile) {
        return Ok(program);
    }
    if let Some(program) = mana_permanents::renowned_weaponsmith(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = mana_permanents::deranged_assistant(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = reductions::starfield_mystic(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = reductions::immortal_sun(card, profile) {
        return Ok(program);
    }
    if let Some(program) = reductions::dryad_of_elysian_grove(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = reductions::skittering_cicada(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = reductions::raff_capashen(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = reductions::padeem(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = mana_permanents::ashnods_altar(card, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::feast(card, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::jailer(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::upkeep(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = monarch::regal_sliver(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = granted_mana::compile(card, vocabulary, profile) {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = token_activations::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = reductions::additional_land_plays(card, vocabulary)
    {
        return Ok(program);
    }
    if let Some(program) = paid_color_mana::compile(card, vocabulary, profile) {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = artifact_activations::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = flash_permissions::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if let Some(program) = power_mana::compile(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = counted_mana::compile(card, vocabulary, profile) {
        return Ok(program);
    }
    if let Some(program) = hybrid_filter::compile(card, profile) {
        return Ok(program);
    }
    if let Some(program) = conditional_mana::compile(card, profile) {
        return Ok(program);
    }
    if let Some(program) = urza_mana::compile(card, profile) {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = static_grants::graveyard_creature_cast(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = static_grants::unblockable_creature(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = static_grants::blocking_restrictions(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = static_grants::sliver_selected_activation(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = static_grants::sliver_keywords(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = static_grants::sliver_stats(card, vocabulary)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = static_grants::compile(card)
    {
        return Ok(program);
    }
    if profile == Profile::Current
        && let Some(program) = army_grants::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV6
        && let Some(program) = power_up::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV6
        && let Some(program) = station::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV6
        && let Some(program) = attacking_tokens::zombie_creator(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV5
        && let Some(program) = ninja::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV5
        && let Some(program) = partner_cast_trigger(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV5
        && let Some(program) = evoke::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV5
        && let Some(land) = basic_lands::compile(card, profile)
    {
        return Ok(land);
    }
    if profile >= Profile::GameplayV5
        && let Some(program) = attachments::compile_bestow(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV4
        && let Some(program) = face_down::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV3
        && let Some(program) = escape::attachment(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV3
        && let Some(program) = limited_activations::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile >= Profile::GameplayV3
        && let Some(program) = crew_saddle::compile(card, vocabulary)
    {
        return Ok(program);
    }
    if profile != Profile::CreatureV1
        && let Some(program) = attachments::compile(card, vocabulary, profile)
            .or_else(|| spells::compile(card, vocabulary, profile))
    {
        return Ok(program);
    }
    ordinary_creature(card, vocabulary, profile, None).or_else(|reason| {
        if profile == Profile::Current {
            ordinary_triggers::compile(card, vocabulary)
                .or_else(|| source_boost::compile(card, vocabulary, None))
                .or_else(|| mana_permanents::compile(card, vocabulary))
                .or_else(|| avacyn_restored::compile(card, vocabulary))
                .or_else(|| innistrad::compile(card, vocabulary))
                .or_else(|| oracle_lowering::compile(card, vocabulary))
                .ok_or(reason)
        } else {
            Err(reason)
        }
    })
}

fn transfigure(card: &InventoryCard, vocabulary: &CreatureTypeVocabulary) -> Option<Program> {
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let cost = fixed_cost(text.trim().strip_prefix("Transfigure ")?)?;
    let mut program = ordinary_creature(card, vocabulary, Profile::Current, Some("")).ok()?;
    use spellwake_engine::activated::{ActivationRestriction, SourceCost};
    use spellwake_engine::program::{
        BodyTarget, ExecutableBody, Instruction, LibrarySearch, SearchBattlefieldPostEntry,
        SearchCardMatcher, SearchDestination, SearchManaValue,
    };
    let id = ActivatedAbilityId(0);
    program.activated.push(ActivatedAbility {
        id,
        mana_cost: cost,
        source_cost: SourceCost::Sacrifice,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::SearchLibrary {
                filter: Box::new(
                    LibrarySearch::to_battlefield(
                        SearchCardMatcher::CardTypes([CardType::Creature].into()),
                        false,
                    )
                    .with_mana_value(SearchManaValue::Exact(program.mana_value()))
                    .with_destination(SearchDestination::Battlefield {
                        tapped: false,
                        post_entry: SearchBattlefieldPostEntry::Transfigure,
                    }),
                ),
            }],
        ),
    });
    program
        .activation_restrictions
        .insert(id, ActivationRestriction::Sorcery);
    program.validate(vocabulary).ok()?;
    Some(program)
}
fn partner_cast_trigger(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Program> {
    use spellwake_engine::program::{PlayerRelation, TriggerGrouping};
    let [face] = card.faces.as_slice() else {
        return None;
    };
    let text = without_reminder(face.oracle_text.as_deref()?)?;
    let mut remaining = Vec::new();
    let mut found = false;
    let short = face.name.split(',').next()?;
    let printed = format!("Whenever an opponent casts a spell, put a +1/+1 counter on {short}.");
    for line in text.lines().map(str::trim) {
        if line == printed {
            if found {
                return None;
            }
            found = true;
        } else {
            remaining.push(line);
        }
    }
    if !found || !remaining.contains(&"Partner") {
        return None;
    }
    let mut program = ordinary_creature(
        card,
        vocabulary,
        Profile::Current,
        Some(&remaining.join("\n")),
    )
    .ok()?;
    let id = TriggeredAbilityId(u64::try_from(program.triggered.len()).ok()?);
    program.triggered.push(TriggeredAbility {
        id,
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            PlayerRelation::Opponent,
            spellwake_engine::program::SpellFilter::any(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::PutCountersOnSource { amount: 1.into() }],
        ),
    });
    Some(program)
}

fn ordinary_creature(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
    remaining_text: Option<&str>,
) -> Result<Program, &'static str> {
    creature_face(card, vocabulary, profile, remaining_text, false)
}
fn creature_face(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
    remaining_text: Option<&str>,
    night_face: bool,
) -> Result<Program, &'static str> {
    let [face] = card.faces.as_slice() else {
        return Err("outside-single-normal-face");
    };
    if card.layout != "normal" || face.name != card.name {
        return Err("outside-single-normal-face");
    }
    let line = face
        .type_line
        .as_deref()
        .ok_or("outside-ordinary-creature-type")?;
    let (types, subtypes) = line.split_once(" — ").unwrap_or((line, ""));
    if !matches!(
        types,
        "Creature"
            | "Artifact Creature"
            | "Enchantment Creature"
            | "Legendary Creature"
            | "Legendary Artifact Creature"
            | "Legendary Enchantment Creature"
            | "Snow Creature"
            | "Snow Artifact Creature"
    ) {
        return Err("outside-ordinary-creature-type");
    }
    let text = remaining_text.or(face.oracle_text.as_deref()).unwrap_or("");
    let abilities = if remaining_text == Some("")
        || (profile >= Profile::GameplayV6 && text.trim().is_empty())
    {
        ParsedKeywords::default()
    } else {
        parse_profile_keywords(
            text,
            (profile != Profile::CreatureV1).then_some(vocabulary),
            face.mana_cost.as_deref(),
            profile,
        )
        .ok_or("requires-other-rules-text")?
    };
    let mana = if night_face && face.mana_cost.as_deref().unwrap_or("").is_empty() {
        None
    } else {
        Some(
            fixed_cost(face.mana_cost.as_deref().unwrap_or(""))
                .ok_or("requires-other-mana-cost")?,
        )
    };
    let power = number(face.power.as_deref()).ok_or("requires-other-power-toughness")?;
    let toughness = number(face.toughness.as_deref()).ok_or("requires-other-power-toughness")?;
    let creature_subtypes = subtypes
        .split_whitespace()
        .map(|label| vocabulary.resolve_label(label))
        .collect::<Option<_>>()
        .ok_or("requires-other-subtype")?;
    let mut program = Program::creature(mana, power.into(), toughness.into(), creature_subtypes)
        .with_printed_keywords(abilities.keywords);
    program.entry_counter_abilities = abilities.entry_counters;
    program.plot = abilities.plot;
    program.exile_action = abilities.exile_action;
    program.madness = abilities.madness;
    program.printed_toxic = abilities.toxic;
    program.triggered = abilities.triggers;
    program.continuous = abilities.continuous;
    program.activated = abilities.activated;
    program.conditional_cost = abilities.conditional;
    program.cast_reductions = abilities.reductions;
    for kind in types.split_whitespace() {
        match kind {
            "Artifact" => {
                program.card_types.insert(CardType::Artifact);
            }
            "Enchantment" => {
                program.card_types.insert(CardType::Enchantment);
            }
            "Legendary" => {
                program.supertypes.insert(Supertype::Legendary);
            }
            "Snow" => {
                program.supertypes.insert(Supertype::Snow);
            }
            _ => {}
        }
    }
    Ok(program)
}

fn number(raw: Option<&str>) -> Option<i64> {
    let raw = raw?;
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

fn split_hybrid_cost(raw: &str) -> Option<(String, Vec<spellwake_engine::mana::HybridSymbol>)> {
    use spellwake_engine::mana::{HybridAlternative, HybridSymbol};
    if !raw.contains('/') {
        return None;
    }
    let color = |s| match s {
        "W" => Some(ManaType::White),
        "U" => Some(ManaType::Blue),
        "B" => Some(ManaType::Black),
        "R" => Some(ManaType::Red),
        "G" => Some(ManaType::Green),
        _ => None,
    };
    let mut price = String::new();
    let mut symbols = Vec::new();
    for symbol in raw.strip_prefix('{')?.strip_suffix('}')?.split("}{") {
        let canonical = if let Some((first, other)) = symbol.split_once('/') {
            let alternative = if other == "P" {
                HybridAlternative::TwoLife
            } else {
                HybridAlternative::Mana(color(other)?)
            };
            symbols.push(HybridSymbol {
                first: color(first)?,
                alternative,
            });
            first
        } else {
            symbol
        };
        price.push('{');
        price.push_str(canonical);
        price.push('}');
    }
    Some((price, symbols))
}

fn fixed_cost(raw: &str) -> Option<ManaCost> {
    let mut cost = ManaCost::free();
    let symbols = raw.strip_prefix('{')?.strip_suffix('}')?;
    for symbol in symbols.split("}{") {
        let kind = match symbol {
            "W" => ManaType::White,
            "U" => ManaType::Blue,
            "B" => ManaType::Black,
            "R" => ManaType::Red,
            "G" => ManaType::Green,
            "C" => ManaType::Colourless,
            _ => {
                cost.generic = cost.generic.plus(&number(Some(symbol))?.into());
                continue;
            }
        };
        let amount = cost.required.entry(kind).or_insert_with(|| 0.into());
        *amount = amount.plus(&1.into());
    }
    Some(cost)
}

// CR 207.2a: reminder text in parentheses has no game effect. Admit only an
// entire comma/newline-separated list of named abilities outside that text.
// Parentheses must balance, including on multiline Oracle text.
#[derive(Default)]
struct ParsedKeywords {
    madness: Option<ManaCost>,
    exile_action: Option<spellwake_engine::program::ExileAction>,
    entry_counters: Vec<spellwake_engine::program::EntryCounterAbility>,
    plot: Option<ManaCost>,
    conditional: Option<spellwake_engine::program::ConditionalCastCost>,
    toxic: Option<spellwake_contracts::RulesInteger>,
    continuous: Option<spellwake_engine::characteristics::StaticContinuousAbility>,
    keywords: BTreeSet<KeywordAbility>,
    triggers: Vec<TriggeredAbility>,
    activated: Vec<ActivatedAbility>,
    reductions: Vec<CastReduction>,
}
#[cfg(test)]
fn keyword_text(raw: &str) -> Option<(BTreeSet<KeywordAbility>, Vec<TriggeredAbility>)> {
    parse_keywords(raw, None, None).map(|parsed| (parsed.keywords, parsed.triggers))
}
fn without_reminder(raw: &str) -> Option<String> {
    let mut rules = String::new();
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '(' => {
                depth += 1;
                rules.push(' ');
            }
            ')' => depth = depth.checked_sub(1)?,
            _ if depth == 0 => rules.push(c),
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    Some(rules)
}
fn parse_keywords(
    raw: &str,
    expanded: Option<&CreatureTypeVocabulary>,
    source_cost: Option<&str>,
) -> Option<ParsedKeywords> {
    parse_profile_keywords(raw, expanded, source_cost, Profile::GameplayV2)
}
fn parse_profile_keywords(
    raw: &str,
    expanded: Option<&CreatureTypeVocabulary>,
    source_cost: Option<&str>,
    profile: Profile,
) -> Option<ParsedKeywords> {
    let rules = without_reminder(raw)?;
    let (rules, escape) = escape::extract(rules, profile)?;
    let has_escape = escape.is_some();
    let mut parsed = ParsedKeywords {
        conditional: escape,
        ..Default::default()
    };
    let labels: Vec<_> = rules
        .lines()
        .flat_map(|line| {
            // The comma introducing a cost formula belongs to the same ability.
            let formula = profile == Profile::Current
                && line
                    .trim_start()
                    .starts_with("This spell costs {X} less to cast, where X is ");
            let banding = profile == Profile::Current
                && line.to_ascii_lowercase().contains("banding")
                && !line.contains(':');
            line.split(move |ch| (ch == ',' && !formula) || (ch == ';' && banding))
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    for (index, label) in labels.iter().enumerate() {
        if let Some(vocabulary) = expanded {
            if profile >= Profile::GameplayV5
                && let Some(price) = label.strip_prefix("madness ")
            {
                if parsed
                    .madness
                    .replace(fixed_cost(&price.to_ascii_uppercase())?)
                    .is_some()
                {
                    return None;
                }
                continue;
            }
            if profile >= Profile::GameplayV4
                && let Some(action) = exile_hand::parse(label)
            {
                if parsed.exile_action.replace(action).is_some() {
                    return None;
                }
                continue;
            }
            if profile >= Profile::GameplayV4 {
                use spellwake_engine::program::EntryCounterAbility;
                if profile == Profile::Current
                    && let Some(n) = label.strip_prefix("devour ")
                {
                    if parsed
                        .entry_counters
                        .iter()
                        .any(|rule| matches!(rule, EntryCounterAbility::Devour(_)))
                    {
                        return None;
                    }
                    parsed
                        .entry_counters
                        .push(EntryCounterAbility::Devour(n.parse().ok()?));
                    continue;
                }
                if label == "sunburst" {
                    parsed.entry_counters.push(EntryCounterAbility::Sunburst);
                    continue;
                }
                if let Some(amount) = label.strip_prefix("bloodthirst ") {
                    let ability = if amount == "x" {
                        EntryCounterAbility::BloodthirstX
                    } else {
                        let n =
                            spellwake_contracts::RulesInteger::try_from(amount.to_owned()).ok()?;
                        if n < 0.into() {
                            return None;
                        }
                        EntryCounterAbility::Bloodthirst(n)
                    };
                    parsed.entry_counters.push(ability);
                    continue;
                }
            }

            if profile >= Profile::GameplayV3
                && let Some(price) = label.strip_prefix("plot ")
            {
                if parsed
                    .plot
                    .replace(fixed_cost(&price.to_ascii_uppercase())?)
                    .is_some()
                {
                    return None;
                }
                continue;
            }
            if profile >= Profile::GameplayV3
                && let Some(qualities) = label.strip_prefix("protection from ")
            {
                for word in qualities.split(" and ") {
                    let color = word.strip_prefix("from ").unwrap_or(word);
                    let key = match color {
                        "white" => KeywordAbility::ProtectionFromWhite,
                        "blue" => KeywordAbility::ProtectionFromBlue,
                        "black" => KeywordAbility::ProtectionFromBlack,
                        "red" => KeywordAbility::ProtectionFromRed,
                        "green" => KeywordAbility::ProtectionFromGreen,
                        "multicolored" if profile == Profile::Current => {
                            KeywordAbility::ProtectionFromMulticolored
                        }
                        _ => return None,
                    };
                    parsed.keywords.insert(key);
                }
                continue;
            }
            match label.as_str() {
                "wither" => {
                    parsed.keywords.insert(KeywordAbility::Wither);
                    continue;
                }
                "infect" => {
                    parsed.keywords.insert(KeywordAbility::Infect);
                    continue;
                }
                _ => {}
            }
            if let Some(value) = label.strip_prefix("toxic ") {
                let amount = spellwake_contracts::RulesInteger::try_from(value.to_owned()).ok()?;
                if amount < 0.into() {
                    return None;
                }
                parsed.toxic = Some(
                    parsed
                        .toxic
                        .map_or_else(|| amount.clone(), |total| total.plus(&amount)),
                );
                continue;
            }

            if let Some(grant) = combat_triggers::sliver_grant(label, vocabulary) {
                if parsed.continuous.replace(grant).is_some() {
                    return None;
                }
                continue;
            }

            if let Some(price) = reductions::conditional(label, profile) {
                if parsed.conditional.replace(price).is_some() {
                    return None;
                }
                continue;
            }
            if let Some(reduction) = reductions::parse(label, vocabulary, profile) {
                parsed.reductions.push(reduction);
                continue;
            }
            let trigger_id = TriggeredAbilityId(u64::try_from(parsed.triggers.len()).ok()?);
            if profile >= Profile::GameplayV6 && label == "cascade" {
                parsed.triggers.push(TriggeredAbility::cascade(trigger_id));
                continue;
            }
            if profile >= Profile::GameplayV6
                && let Some(keyword) = attacking_tokens::parse(label)
            {
                if keyword == spellwake_engine::program::CombatTriggerKeyword::Decayed {
                    parsed.keywords.insert(KeywordAbility::Decayed);
                }
                parsed
                    .triggers
                    .push(TriggeredAbility::combat_keyword(trigger_id, keyword));
                continue;
            }
            if profile >= Profile::GameplayV5 {
                if label == "ingest" {
                    parsed.triggers.push(TriggeredAbility::combat_keyword(
                        trigger_id,
                        spellwake_engine::program::CombatTriggerKeyword::Ingest,
                    ));
                    continue;
                }
                if let Some(amount) = label.strip_prefix("annihilator ") {
                    parsed.triggers.push(TriggeredAbility::annihilator(
                        trigger_id,
                        number(Some(amount))?.into(),
                    ));
                    continue;
                }
            }
            if profile >= Profile::GameplayV5 && label == "extort" {
                parsed.triggers.push(TriggeredAbility::extort(trigger_id));
                continue;
            }
            if profile >= Profile::GameplayV5
                && let Some(key) = transfers::parse(label)
            {
                parsed.entry_counters.extend(key.entry());
                parsed.triggers.push(key.ability(trigger_id));
                continue;
            }
            if profile >= Profile::GameplayV5
                && let Some(ability) = growth::trigger(label, &labels[index + 1..], trigger_id)
            {
                parsed.triggers.push(ability);
                continue;
            }
            if profile >= Profile::GameplayV4
                && let Some(abilities) = counter_keywords::triggers(label, trigger_id)
            {
                parsed.triggers.extend(abilities);
                continue;
            }
            if profile >= Profile::GameplayV3
                && let Some(ability) = advanced_combat::trigger(label, trigger_id)
                    .or_else(|| upkeep::trigger(label, trigger_id))
            {
                parsed.triggers.push(ability);
                continue;
            }
            if profile >= Profile::GameplayV3
                && let Some(price) = label.strip_prefix("ward ")
            {
                parsed.triggers.push(TriggeredAbility::ward(
                    trigger_id,
                    fixed_cost(&price.to_ascii_uppercase())?,
                ));
                continue;
            }
            if let Some((word, operand)) = label.split_once(' ')
                && let Some(keyword) = match word {
                    "afterlife" => Some(spellwake_engine::program::TokenKeyword::Afterlife),
                    "fabricate" => Some(spellwake_engine::program::TokenKeyword::Fabricate),
                    _ => None,
                }
            {
                let amount =
                    spellwake_contracts::RulesInteger::try_from(operand.to_owned()).ok()?;
                parsed
                    .triggers
                    .push(keyword.ability(trigger_id, amount, vocabulary).ok()?);
                continue;
            }
            if let Some(trigger) = combat_triggers::combat_trigger(label, trigger_id) {
                parsed.triggers.push(trigger);
                continue;
            }
            let activation_id = ActivatedAbilityId(u64::try_from(parsed.activated.len()).ok()?);
            if profile >= Profile::GameplayV3
                && let Some(ability) = cycling::extended_hand_ability(label, activation_id)
            {
                parsed.activated.push(ability);
                continue;
            }
            if let Some(ability) =
                cycling::hand_ability(label, activation_id, vocabulary, source_cost)
            {
                parsed.activated.push(ability);
                continue;
            }
        }
        let keyword = match label.as_str() {
            "phasing" if profile == Profile::Current => KeywordAbility::Phasing,
            "partner" if profile >= Profile::GameplayV5 => KeywordAbility::Partner,
            "convoke" if expanded.is_some() => KeywordAbility::Convoke,
            "delve" if expanded.is_some() => KeywordAbility::Delve,
            "improvise" if expanded.is_some() => KeywordAbility::Improvise,
            "exalted" => {
                let id = TriggeredAbilityId(u64::try_from(parsed.triggers.len()).ok()?);
                parsed.triggers.push(TriggeredAbility::exalted(id));
                continue;
            }
            "banding" if profile != Profile::Current => return None,
            _ => static_keyword(label)?,
        };
        if !parsed.keywords.insert(keyword) {
            return None;
        }
    }
    (has_escape
        || (profile >= Profile::GameplayV6 && parsed.conditional.is_some())
        || parsed.exile_action.is_some()
        || parsed.madness.is_some()
        || !parsed.entry_counters.is_empty()
        || parsed.plot.is_some()
        || parsed.toxic.is_some()
        || parsed.continuous.is_some()
        || !parsed.keywords.is_empty()
        || !parsed.triggers.is_empty()
        || !parsed.reductions.is_empty()
        || !parsed.activated.is_empty())
    .then_some(parsed)
}

fn static_keyword(label: &str) -> Option<KeywordAbility> {
    Some(match label {
        "haste" => KeywordAbility::Haste,
        "banding" => KeywordAbility::Banding,
        "vigilance" => KeywordAbility::Vigilance,
        "flying" => KeywordAbility::Flying,
        "reach" => KeywordAbility::Reach,
        "defender" => KeywordAbility::Defender,
        "menace" => KeywordAbility::Menace,
        "shadow" => KeywordAbility::Shadow,
        "horsemanship" => KeywordAbility::Horsemanship,
        "fear" => KeywordAbility::Fear,
        "intimidate" => KeywordAbility::Intimidate,
        "skulk" => KeywordAbility::Skulk,
        "plainswalk" => KeywordAbility::Plainswalk,
        "islandwalk" => KeywordAbility::Islandwalk,
        "swampwalk" => KeywordAbility::Swampwalk,
        "mountainwalk" => KeywordAbility::Mountainwalk,
        "forestwalk" => KeywordAbility::Forestwalk,
        "desertwalk" => KeywordAbility::Desertwalk,
        "nonbasic landwalk" => KeywordAbility::NonbasicLandwalk,
        "legendary landwalk" => KeywordAbility::LegendaryLandwalk,
        "snow landwalk" => KeywordAbility::SnowLandwalk,
        "snow forestwalk" => KeywordAbility::SnowForestwalk,
        "snow swampwalk" => KeywordAbility::SnowSwampwalk,
        "artifact landwalk" => KeywordAbility::ArtifactLandwalk,
        "trample" => KeywordAbility::Trample,
        "deathtouch" => KeywordAbility::Deathtouch,
        "lifelink" => KeywordAbility::Lifelink,
        "indestructible" => KeywordAbility::Indestructible,
        "first strike" => KeywordAbility::FirstStrike,
        "double strike" => KeywordAbility::DoubleStrike,
        "shroud" => KeywordAbility::Shroud,
        "hexproof" => KeywordAbility::Hexproof,
        "flash" => KeywordAbility::Flash,
        "split second" => KeywordAbility::SplitSecond,
        "changeling" => KeywordAbility::Changeling,
        "devoid" => KeywordAbility::Devoid,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[allow(dead_code)] // Shared strict loader also exposes wire-view helpers to other consumers.
#[path = "../../engine/test_support/creature_vocabulary.rs"]
mod vocabulary;
