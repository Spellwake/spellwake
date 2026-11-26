//! Exact player shape to typed kernel input. No alternate rules execution.
use super::*;
use crate::player_wire::{ManaColor, PlayerAction, PlayerChoice, PlayerPayment, PlayerTarget};
use spellwake_engine::{
    checks::CheckDecision,
    combat::{AttackDeclaration, BlockDeclaration, CombatDamageAssignment, CombatDecision},
    damage::{DamageAllocation, DamageRecipient},
    effects::{DamageDecision, ResolutionPaymentReply},
    game::{DecisionReply, GameCommand, RuleFrame},
    mana::{ManaPayment, ManaSpend, ManaType},
    object_identity::ObjectId,
    opening::OpeningDecision,
    targets::TargetRef,
};

fn native_card_type(
    value: crate::player_wire::PlayerCardType,
) -> spellwake_engine::program::CardType {
    use crate::player_wire::PlayerCardType as Wire;
    use spellwake_engine::program::CardType;
    match value {
        Wire::Battle => CardType::Battle,
        Wire::Land => CardType::Land,
        Wire::Creature => CardType::Creature,
        Wire::Instant => CardType::Instant,
        Wire::Sorcery => CardType::Sorcery,
        Wire::Planeswalker => CardType::Planeswalker,
        Wire::Artifact => CardType::Artifact,
        Wire::Enchantment => CardType::Enchantment,
        Wire::Kindred => CardType::Kindred,
    }
}
fn native_mana_color(color: ManaColor) -> ManaType {
    match color {
        ManaColor::White => ManaType::White,
        ManaColor::Blue => ManaType::Blue,
        ManaColor::Black => ManaType::Black,
        ManaColor::Red => ManaType::Red,
        ManaColor::Green => ManaType::Green,
        ManaColor::Colourless => ManaType::Colourless,
    }
}
fn apply_cast_face(
    payment: &mut spellwake_engine::mana::CastingPayment,
    face: Option<crate::player_wire::PlayerCastFace>,
) {
    use crate::player_wire::PlayerCastFace;
    use spellwake_engine::program::SplitHalf;
    payment.split_half = match face {
        Some(PlayerCastFace::SplitLeft) => Some(SplitHalf::Left),
        Some(PlayerCastFace::SplitRight) => Some(SplitHalf::Right),
        _ => None,
    };
    payment.disturb = face == Some(PlayerCastFace::Disturb);
    payment.modal_back = face == Some(PlayerCastFace::ModalBack);
    payment.prototype = face == Some(PlayerCastFace::Prototype);
    payment.prepare = face == Some(PlayerCastFace::Prepare);
}
fn hybrid_side(value: &crate::player_wire::PlayerHybridSide) -> spellwake_engine::mana::HybridSide {
    match value {
        crate::player_wire::PlayerHybridSide::First => spellwake_engine::mana::HybridSide::First,
        crate::player_wire::PlayerHybridSide::Second => spellwake_engine::mana::HybridSide::Second,
    }
}

fn payment(value: &PlayerPayment) -> ManaPayment {
    ManaPayment {
        from_lots: value
            .from_lots
            .iter()
            .map(|(&id, n)| (id, n.as_rules_integer().clone()))
            .collect(),
        spend: value
            .spend
            .iter()
            .map(|entry| ManaSpend {
                mana_type: native_mana_color(entry.color),
                amount: entry.amount.as_rules_integer().clone(),
            })
            .collect(),
    }
}
fn plain_payment(value: &PlayerPayment) -> Result<ManaPayment, NativeBridgeError> {
    if !value.restricted_mana.is_empty()
        || value.exile_cast_type.is_some()
        || value.graveyard_permission.is_some()
        || value.alternative_cost_source.is_some()
    {
        return Err(NativeBridgeError::InvalidIntent);
    }
    Ok(payment(value))
}
fn restricted_payment(
    value: &PlayerPayment,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Result<Vec<spellwake_engine::mana::RestrictedManaSpend>, NativeBridgeError> {
    value
        .restricted_mana
        .iter()
        .map(|entry| {
            Ok(spellwake_engine::mana::RestrictedManaSpend {
                mana_type: native_mana_color(entry.color),
                restriction: match entry.restriction {
                    crate::player_wire::PlayerManaRestriction::PlaneswalkerSpell => {
                        spellwake_engine::mana::ManaRestriction::PlaneswalkerSpell
                    }
                    crate::player_wire::PlayerManaRestriction::ColorlessSpell => {
                        spellwake_engine::mana::ManaRestriction::ColorlessSpell
                    }
                    crate::player_wire::PlayerManaRestriction::CommanderSpell => {
                        spellwake_engine::mana::ManaRestriction::CommanderSpell
                    }
                    crate::player_wire::PlayerManaRestriction::ColorlessEldraziSpellOrAbility => {
                        spellwake_engine::mana::ManaRestriction::ColorlessEldraziSpellOrAbility
                    }
                    crate::player_wire::PlayerManaRestriction::SpellWithFlashbackFromGraveyard => {
                        spellwake_engine::mana::ManaRestriction::SpellWithFlashbackFromGraveyard
                    }
                    crate::player_wire::PlayerManaRestriction::NoncreatureSpell => {
                        spellwake_engine::mana::ManaRestriction::NoncreatureSpell
                    }
                    crate::player_wire::PlayerManaRestriction::ArtifactSpellOrAbility => {
                        spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAbility
                    }
                    crate::player_wire::PlayerManaRestriction::NotNonartifactSpell => {
                        spellwake_engine::mana::ManaRestriction::NotNonartifactSpell
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureSpell => {
                        spellwake_engine::mana::ManaRestriction::CreatureSpell
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureSpellOfType => {
                        spellwake_engine::mana::ManaRestriction::CreatureSpellOfType(
                            entry
                                .creature_type
                                .as_ref()
                                .map(|label| {
                                    vocabulary
                                        .resolve_label(label.as_str())
                                        .ok_or(NativeBridgeError::InvalidIntent)
                                })
                                .transpose()?,
                        )
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureSpellOfTypeUncounterable => {
                        spellwake_engine::mana::ManaRestriction::CreatureSpellOfTypeUncounterable(
                            entry
                                .creature_type
                                .as_ref()
                                .map(|label| {
                                    vocabulary
                                        .resolve_label(label.as_str())
                                        .ok_or(NativeBridgeError::InvalidIntent)
                                })
                                .transpose()?,
                        )
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureSpellOrAbilityOfType => {
                        spellwake_engine::mana::ManaRestriction::CreatureSpellOrAbilityOfType(
                            entry
                                .creature_type
                                .as_ref()
                                .map(|label| {
                                    vocabulary
                                        .resolve_label(label.as_str())
                                        .ok_or(NativeBridgeError::InvalidIntent)
                                })
                                .transpose()?,
                        )
                    }
                    crate::player_wire::PlayerManaRestriction::InstantOrSorcerySpell => {
                        spellwake_engine::mana::ManaRestriction::InstantOrSorcerySpell
                    }
                    crate::player_wire::PlayerManaRestriction::ArtifactSpell => {
                        spellwake_engine::mana::ManaRestriction::ArtifactSpell
                    }
                    crate::player_wire::PlayerManaRestriction::ArtifactAbility => {
                        spellwake_engine::mana::ManaRestriction::ArtifactAbility
                    }
                    crate::player_wire::PlayerManaRestriction::ArtifactSpellOrAnyAbility => {
                        spellwake_engine::mana::ManaRestriction::ArtifactSpellOrAnyAbility
                    }
                    crate::player_wire::PlayerManaRestriction::ActivatedAbility => {
                        spellwake_engine::mana::ManaRestriction::ActivatedAbility
                    }
                    crate::player_wire::PlayerManaRestriction::CastSpell => {
                        spellwake_engine::mana::ManaRestriction::CastSpell
                    }
                    crate::player_wire::PlayerManaRestriction::NotSpellFromHand => {
                        spellwake_engine::mana::ManaRestriction::NotSpellFromHand
                    }
                    crate::player_wire::PlayerManaRestriction::CostContainingX => {
                        spellwake_engine::mana::ManaRestriction::CostContainingX
                    }
                    crate::player_wire::PlayerManaRestriction::LegendarySpell => {
                        spellwake_engine::mana::ManaRestriction::LegendarySpell
                    }
                    crate::player_wire::PlayerManaRestriction::MulticoloredSpell => {
                        spellwake_engine::mana::ManaRestriction::MulticoloredSpell
                    }
                    crate::player_wire::PlayerManaRestriction::EnchantmentSpell => {
                        spellwake_engine::mana::ManaRestriction::EnchantmentSpell
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureSpellOrAbility => {
                        spellwake_engine::mana::ManaRestriction::CreatureSpellOrAbility
                    }
                    crate::player_wire::PlayerManaRestriction::ColoredSpellWithoutX => {
                        spellwake_engine::mana::ManaRestriction::ColoredSpellWithoutX
                    }
                    crate::player_wire::PlayerManaRestriction::InstantOrSorceryOrForetell => {
                        spellwake_engine::mana::ManaRestriction::InstantOrSorceryOrForetell
                    }
                    crate::player_wire::PlayerManaRestriction::SpellWithMinManaValue => {
                        spellwake_engine::mana::ManaRestriction::SpellWithMinManaValue(
                            entry
                                .min_mana_value
                                .ok_or(NativeBridgeError::InvalidIntent)?,
                        )
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureSpellWithMinManaValue => {
                        spellwake_engine::mana::ManaRestriction::CreatureSpellWithMinManaValue(
                            entry
                                .min_mana_value
                                .ok_or(NativeBridgeError::InvalidIntent)?,
                        )
                    }
                    crate::player_wire::PlayerManaRestriction::HistoricSpell => {
                        spellwake_engine::mana::ManaRestriction::HistoricSpell
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureOrPlaneswalkerSpell => {
                        spellwake_engine::mana::ManaRestriction::CreatureOrPlaneswalkerSpell
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureAbility => {
                        spellwake_engine::mana::ManaRestriction::CreatureAbility
                    }
                    crate::player_wire::PlayerManaRestriction::PermanentSpell => {
                        spellwake_engine::mana::ManaRestriction::PermanentSpell
                    }
                    crate::player_wire::PlayerManaRestriction::SpellFromGraveyardOrExile => {
                        spellwake_engine::mana::ManaRestriction::SpellFromGraveyardOrExile
                    }
                    crate::player_wire::PlayerManaRestriction::EquipmentOrEquipAbility => {
                        spellwake_engine::mana::ManaRestriction::EquipmentOrEquipAbility
                    }
                    crate::player_wire::PlayerManaRestriction::SpellWithMinManaValueOrX => {
                        spellwake_engine::mana::ManaRestriction::SpellWithMinManaValueOrX(
                            entry
                                .min_mana_value
                                .ok_or(NativeBridgeError::InvalidIntent)?,
                        )
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureSpellWithMinManaValueOrX => {
                        spellwake_engine::mana::ManaRestriction::CreatureSpellWithMinManaValueOrX(
                            entry
                                .min_mana_value
                                .ok_or(NativeBridgeError::InvalidIntent)?,
                        )
                    }
                    crate::player_wire::PlayerManaRestriction::CreatureOrEnchantmentSpell => {
                        spellwake_engine::mana::ManaRestriction::CreatureOrEnchantmentSpell
                    }
                    crate::player_wire::PlayerManaRestriction::Predicate => {
                        let pred = entry
                            .predicate
                            .as_ref()
                            .ok_or(NativeBridgeError::InvalidIntent)?;
                        spellwake_engine::mana::ManaRestriction::Predicate(
                            translate_mana_predicate(pred, vocabulary)?,
                        )
                    }
                },
                amount: entry.amount.as_rules_integer().clone(),
            })
        })
        .collect()
}

fn translate_mana_color_from_wire(
    color: crate::player_wire::ManaColor,
) -> Result<spellwake_engine::characteristics::Color, NativeBridgeError> {
    match color {
        crate::player_wire::ManaColor::White => Ok(spellwake_engine::characteristics::Color::White),
        crate::player_wire::ManaColor::Blue => Ok(spellwake_engine::characteristics::Color::Blue),
        crate::player_wire::ManaColor::Black => Ok(spellwake_engine::characteristics::Color::Black),
        crate::player_wire::ManaColor::Red => Ok(spellwake_engine::characteristics::Color::Red),
        crate::player_wire::ManaColor::Green => Ok(spellwake_engine::characteristics::Color::Green),
        crate::player_wire::ManaColor::Colourless => Err(NativeBridgeError::InvalidIntent),
    }
}

fn translate_card_type_from_wire(
    kind: crate::player_wire::PlayerCardType,
) -> spellwake_engine::program::CardType {
    match kind {
        crate::player_wire::PlayerCardType::Battle => spellwake_engine::program::CardType::Battle,
        crate::player_wire::PlayerCardType::Planeswalker => {
            spellwake_engine::program::CardType::Planeswalker
        }
        crate::player_wire::PlayerCardType::Land => spellwake_engine::program::CardType::Land,
        crate::player_wire::PlayerCardType::Artifact => {
            spellwake_engine::program::CardType::Artifact
        }
        crate::player_wire::PlayerCardType::Creature => {
            spellwake_engine::program::CardType::Creature
        }
        crate::player_wire::PlayerCardType::Enchantment => {
            spellwake_engine::program::CardType::Enchantment
        }
        crate::player_wire::PlayerCardType::Kindred => spellwake_engine::program::CardType::Kindred,
        crate::player_wire::PlayerCardType::Instant => spellwake_engine::program::CardType::Instant,
        crate::player_wire::PlayerCardType::Sorcery => spellwake_engine::program::CardType::Sorcery,
    }
}

fn translate_mana_predicate(
    pred: &crate::player_wire::PlayerManaPredicate,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Result<spellwake_engine::mana::ManaSpendPredicate, NativeBridgeError> {
    use crate::player_wire::{
        PlayerAbilitySpendKind, PlayerManaSpecialAction, PlayerManaSpellKeyword, PlayerSpellCastMode,
        PlayerSpellColorRule,
    };
    use spellwake_engine::mana::{
        AbilityManaClause, AbilitySpendKind, ManaSpecialAction, ManaSpellKeyword,
        ManaSpendPredicate, SpellCastMode, SpellColorRule, SpellManaClause,
    };
    let spell_clauses = pred
        .spell_clauses
        .iter()
        .map(|c| {
            Ok(SpellManaClause {
                card_type: c.card_type.map(translate_card_type_from_wire),
                exclude_card_type: c.exclude_card_type.map(translate_card_type_from_wire),
                supertype: c
                    .supertype
                    .as_deref()
                    .map(|s| {
                        spellwake_engine::program::Supertype::from_wire_label(s)
                            .ok_or(NativeBridgeError::InvalidIntent)
                    })
                    .transpose()?,
                creature_type: c
                    .creature_type
                    .as_ref()
                    .map(|l| {
                        vocabulary
                            .resolve_label(l.as_str())
                            .ok_or(NativeBridgeError::InvalidIntent)
                    })
                    .transpose()?,
                artifact_subtype: c
                    .artifact_subtype
                    .as_deref()
                    .map(|s| {
                        spellwake_engine::program::ArtifactSubtype::from_wire_label(s)
                            .ok_or(NativeBridgeError::InvalidIntent)
                    })
                    .transpose()?,
                enchantment_subtype: c
                    .enchantment_subtype
                    .as_deref()
                    .map(|s| {
                        spellwake_engine::program::EnchantmentSubtype::from_wire_label(s)
                            .ok_or(NativeBridgeError::InvalidIntent)
                    })
                    .transpose()?,
                is_aura: c.is_aura,
                spell_subtype: c
                    .spell_subtype
                    .as_deref()
                    .map(|s| {
                        spellwake_engine::program::SpellSubtype::from_wire_label(s)
                            .ok_or(NativeBridgeError::InvalidIntent)
                    })
                    .transpose()?,
                planeswalker_subtype: c
                    .planeswalker_subtype
                    .as_deref()
                    .map(|s| {
                        spellwake_engine::program::PlaneswalkerSubtype::from_label(s)
                            .ok_or(NativeBridgeError::InvalidIntent)
                    })
                    .transpose()?,
                color_rule: c
                    .color_rule
                    .map(|r| match r {
                        PlayerSpellColorRule::Colorless => Ok(SpellColorRule::Colorless),
                        PlayerSpellColorRule::Colored => Ok(SpellColorRule::Colored),
                        PlayerSpellColorRule::ExactColor { color } => {
                            Ok(SpellColorRule::ExactColor(translate_mana_color_from_wire(
                                color,
                            )?))
                        }
                        PlayerSpellColorRule::MonocoloredOf { color } => Ok(
                            SpellColorRule::MonocoloredOf(translate_mana_color_from_wire(color)?),
                        ),
                        PlayerSpellColorRule::Multicolored => Ok(SpellColorRule::Multicolored),
                        PlayerSpellColorRule::ExactColorCount { count } => {
                            Ok(SpellColorRule::ExactColorCount(count))
                        }
                    })
                    .transpose()?,
                keyword: c.keyword.map(|k| match k {
                    PlayerManaSpellKeyword::Devoid => ManaSpellKeyword::Devoid,
                    PlayerManaSpellKeyword::Freerunning => ManaSpellKeyword::Freerunning,
                    PlayerManaSpellKeyword::Foretell => ManaSpellKeyword::Foretell,
                    PlayerManaSpellKeyword::Flashback => ManaSpellKeyword::Flashback,
                    PlayerManaSpellKeyword::Disturb => ManaSpellKeyword::Disturb,
                }),
                cast_mode: c.cast_mode.map(|m| match m {
                    PlayerSpellCastMode::FaceDown => SpellCastMode::FaceDown,
                    PlayerSpellCastMode::Kicked => SpellCastMode::Kicked,
                    PlayerSpellCastMode::Disturb => SpellCastMode::Disturb,
                    PlayerSpellCastMode::FromGraveyard => SpellCastMode::FromGraveyard,
                    PlayerSpellCastMode::FromExile => SpellCastMode::FromExile,
                    PlayerSpellCastMode::NotFromHand => SpellCastMode::NotFromHand,
                    PlayerSpellCastMode::NotOwned => SpellCastMode::NotOwned,
                    PlayerSpellCastMode::SecondSpellThisTurn => SpellCastMode::SecondSpellThisTurn,
                }),
                no_abilities: c.no_abilities,
                min_mana_value: c.min_mana_value,
            })
        })
        .collect::<Result<Vec<_>, NativeBridgeError>>()?;
    let ability_clauses = pred
        .ability_clauses
        .iter()
        .map(|c| {
            Ok(AbilityManaClause {
                source_card_type: c.source_card_type.map(translate_card_type_from_wire),
                source_creature_type: c
                    .source_creature_type
                    .as_ref()
                    .map(|l| {
                        vocabulary
                            .resolve_label(l.as_str())
                            .ok_or(NativeBridgeError::InvalidIntent)
                    })
                    .transpose()?,
                source_colorless: c.source_colorless,
                kind: c.kind.map(|k| match k {
                    PlayerAbilitySpendKind::Equip => AbilitySpendKind::Equip,
                    PlayerAbilitySpendKind::PowerUp => AbilitySpendKind::PowerUp,
                    PlayerAbilitySpendKind::GainClassLevel => AbilitySpendKind::GainClassLevel,
                }),
            })
        })
        .collect::<Result<Vec<_>, NativeBridgeError>>()?;
    let special_actions = pred
        .special_actions
        .iter()
        .map(|a| match a {
            PlayerManaSpecialAction::Foretell => ManaSpecialAction::Foretell,
            PlayerManaSpecialAction::TurnFaceUp => ManaSpecialAction::TurnFaceUp,
            PlayerManaSpecialAction::TurnCreatureFaceUp => ManaSpecialAction::TurnCreatureFaceUp,
            PlayerManaSpecialAction::UnlockDoor => ManaSpecialAction::UnlockDoor,
            PlayerManaSpecialAction::CumulativeUpkeep => ManaSpecialAction::CumulativeUpkeep,
        })
        .collect();
    let mut out = ManaSpendPredicate {
        allow_any_spell: pred.allow_any_spell,
        spell_clauses,
        allow_any_ability: pred.allow_any_ability,
        ability_clauses,
        special_actions,
        makes_spell_uncounterable: pred.makes_spell_uncounterable,
        disallow_generic: pred.disallow_generic,
        allow_cost_containing_colorless: pred.allow_cost_containing_colorless,
        allow_cost_containing_x: pred.allow_cost_containing_x,
    };
    out.canonicalize();
    Ok(out)
}
fn bundle(
    value: &PlayerPayment,
) -> Result<
    std::collections::BTreeMap<ManaType, spellwake_contracts::RulesInteger>,
    NativeBridgeError,
> {
    if !value.from_lots.is_empty() {
        return Err(NativeBridgeError::InvalidIntent);
    }
    let spend = plain_payment(value)?.spend;
    let count = spend.len();
    let values = spend
        .into_iter()
        .map(|entry| (entry.mana_type, entry.amount))
        .collect::<std::collections::BTreeMap<_, _>>();
    if values.len() != count {
        return Err(NativeBridgeError::InvalidIntent);
    }
    Ok(values)
}
impl NativeScope<'_> {
    fn granted_activation(
        &self,
        source: &PlayerHandle,
        ability: &PlayerHandle,
        origin: spellwake_engine::activated::ActivationOrigin,
        chosen_x: Option<spellwake_contracts::RulesInteger>,
        allocation: Option<&PlayerPayment>,
    ) -> Result<Option<GameCommand>, NativeBridgeError> {
        let NativeReference::GrantedAbility {
            source: bound,
            effect,
        } = self.reference(ability)?
        else {
            return Ok(None);
        };
        let source = self.object(source)?;
        if bound != source {
            return Err(NativeBridgeError::InvalidReference);
        }
        if allocation.is_some_and(|p| {
            p.exile_cast_type.is_some()
                || p.graveyard_permission.is_some()
                || p.alternative_cost_source.is_some()
        }) {
            return Err(NativeBridgeError::InvalidIntent);
        }
        Ok(Some(GameCommand::ActivateWithRestrictedMana {
            source,
            ability: spellwake_engine::activated::ActivatedAbilityId(0),
            granting_effect: Some(effect),
            origin,
            chosen_x,
            payment: allocation.map(payment),
            restricted_mana: allocation
                .map(|p| restricted_payment(p, self.source.execution().creature_vocabulary()))
                .transpose()?
                .unwrap_or_default(),
        }))
    }
    fn restricted_ability(
        &self,
        source: &PlayerHandle,
        ability: &PlayerHandle,
        target: Option<&PlayerTarget>,
        objects: Option<&[PlayerHandle]>,
        chosen_x: Option<spellwake_contracts::RulesInteger>,
        allocation: &PlayerPayment,
    ) -> Result<GameCommand, NativeBridgeError> {
        if allocation.exile_cast_type.is_some()
            || allocation.graveyard_permission.is_some()
            || allocation.alternative_cost_source.is_some()
        {
            return Err(NativeBridgeError::InvalidIntent);
        }
        let source_id = self.object(source)?;
        let (ability_id, granting_effect) = match self.reference(ability)? {
            NativeReference::GrantedAbility {
                source: bound,
                effect,
            } if bound == source_id => (
                spellwake_engine::activated::ActivatedAbilityId(0),
                Some(effect),
            ),
            NativeReference::ActivatedAbility {
                source: bound,
                ability,
            } if bound == source_id => (ability, None),
            _ => return Err(NativeBridgeError::WrongReferenceKind),
        };
        let target = target.map(|value| self.target(value)).transpose()?;
        let origin = match objects {
            Some(values) => spellwake_engine::activated::ActivationOrigin::SelectedCost {
                target,
                objects: self.objects(values)?,
            },
            None => target.map_or(
                spellwake_engine::activated::ActivationOrigin::Untargeted,
                spellwake_engine::activated::ActivationOrigin::Targeted,
            ),
        };
        Ok(GameCommand::ActivateWithRestrictedMana {
            source: source_id,
            ability: ability_id,
            granting_effect,
            origin,
            chosen_x,
            payment: Some(payment(allocation)),
            restricted_mana: restricted_payment(
                allocation,
                self.source.execution().creature_vocabulary(),
            )?,
        })
    }
    fn restricted_special_ability(
        &self,
        source: &PlayerHandle,
        ability: &PlayerHandle,
        origin: spellwake_engine::activated::ActivationOrigin,
        allocation: &PlayerPayment,
    ) -> Result<GameCommand, NativeBridgeError> {
        let mut command = self.restricted_ability(source, ability, None, None, None, allocation)?;
        let GameCommand::ActivateWithRestrictedMana { origin: actual, .. } = &mut command else {
            return Err(NativeBridgeError::InvalidIntent);
        };
        *actual = origin;
        Ok(command)
    }
    fn mana_ability(
        &self,
        source: &PlayerHandle,
        ability: &PlayerHandle,
    ) -> Result<(ObjectId, spellwake_engine::mana::ManaAbilityRef), NativeBridgeError> {
        let source = self.object(source)?;
        let NativeReference::ManaAbility {
            source: bound_source,
            ability,
        } = self.reference(ability)?
        else {
            return Err(NativeBridgeError::WrongReferenceKind);
        };
        if source != bound_source {
            return Err(NativeBridgeError::InvalidReference);
        }
        Ok((source, ability))
    }
    pub(super) fn translate_action(
        &self,
        action: &PlayerAction,
    ) -> Result<GameCommand, NativeBridgeError> {
        Ok(match action {
            PlayerAction::CastFaceDown {
                source,
                appearance,
                payment: price,
            } => GameCommand::CastFaceDown {
                spell: self.object(source)?,
                kind: match appearance {
                    crate::player_wire::PlayerFaceDownKind::Morph => {
                        spellwake_engine::face_down::FaceDownKind::Morph
                    }
                    crate::player_wire::PlayerFaceDownKind::Forest
                    | crate::player_wire::PlayerFaceDownKind::Manifest => {
                        return Err(NativeBridgeError::InvalidIntent);
                    }
                    crate::player_wire::PlayerFaceDownKind::Disguise => {
                        spellwake_engine::face_down::FaceDownKind::Disguise
                    }
                },
                payment: price.as_ref().map(plain_payment).transpose()?,
            },
            PlayerAction::TurnFaceUp {
                source,
                ability,
                payment: price,
            } => GameCommand::TurnFaceUp {
                permanent: self.object(source)?,
                ability: usize::from(*ability),
                payment: price.as_ref().map(plain_payment).transpose()?,
            },
            PlayerAction::CastWithResources {
                face,
                adventure,
                hybrid_choices,
                fixed_targets,
                permanent_targets,
                target_division,
                chosen_x,
                modes,
                source,
                target,
                payment: allocation,
                resources,
                additional,
                use_alternative,
            } => GameCommand::CastWithResources {
                spell: self.object(source)?,
                target: target.as_ref().map(|t| self.target(t)).transpose()?,
                payment: {
                    let mut payment = self.cast_payment(
                        *use_alternative,
                        allocation.as_ref(),
                        resources,
                        additional,
                    )?;
                    payment.adventure = *adventure;
                    apply_cast_face(&mut payment, *face);
                    payment.hybrid_choices = hybrid_choices.iter().map(hybrid_side).collect();
                    payment.fixed_targets = fixed_targets
                        .iter()
                        .map(|t| self.target(t))
                        .collect::<Result<_, _>>()?;
                    payment.permanent_targets = permanent_targets
                        .iter()
                        .map(|t| self.target(t))
                        .collect::<Result<_, _>>()?;
                    payment.target_division = target_division.clone();
                    payment.chosen_x = chosen_x.as_ref().map(|x| x.as_rules_integer().clone());
                    payment.modes = modes
                        .iter()
                        .map(|m| {
                            Ok(spellwake_engine::mana::CastMode {
                                mode: m.mode,
                                targets: m
                                    .targets
                                    .iter()
                                    .map(|t| self.target(t))
                                    .collect::<Result<_, _>>()?,
                                target: m.target.as_ref().map(|t| self.target(t)).transpose()?,
                            })
                        })
                        .collect::<Result<_, NativeBridgeError>>()?;
                    payment
                },
            },
            PlayerAction::Foretell {
                source,
                payment: allocation,
            } => GameCommand::Foretell {
                source: self.object(source)?,
                payment: allocation.as_ref().map(plain_payment).transpose()?,
            },
            PlayerAction::Suspend {
                source,
                payment: allocation,
            } => GameCommand::Suspend {
                source: self.object(source)?,
                payment: allocation.as_ref().map(plain_payment).transpose()?,
            },
            PlayerAction::Plot {
                source,
                payment: allocation,
            } => GameCommand::Plot {
                source: self.object(source)?,
                payment: allocation.as_ref().map(plain_payment).transpose()?,
            },
            PlayerAction::CastUntargeted {
                source,
                payment: allocation,
            } => {
                if allocation.as_ref().is_some_and(|value| {
                    !value.restricted_mana.is_empty()
                        || value.exile_cast_type.is_some()
                        || value.graveyard_permission.is_some()
                        || value.alternative_cost_source.is_some()
                }) {
                    GameCommand::CastWithResources {
                        spell: self.object(source)?,
                        target: None,
                        payment: self.cast_payment(false, allocation.as_ref(), &[], &[])?,
                    }
                } else {
                    GameCommand::CastUntargeted {
                        spell: self.object(source)?,
                        payment: allocation.as_ref().map(plain_payment).transpose()?,
                    }
                }
            }
            PlayerAction::CastTargeted {
                source,
                target,
                payment: allocation,
            } => match allocation {
                None => GameCommand::Cast {
                    spell: self.object(source)?,
                    target: self.target(target)?,
                },
                Some(allocation)
                    if !allocation.restricted_mana.is_empty()
                        || allocation.exile_cast_type.is_some()
                        || allocation.graveyard_permission.is_some()
                        || allocation.alternative_cost_source.is_some() =>
                {
                    GameCommand::CastWithResources {
                        spell: self.object(source)?,
                        target: Some(self.target(target)?),
                        payment: self.cast_payment(false, Some(allocation), &[], &[])?,
                    }
                }
                Some(allocation) => GameCommand::CastPaid {
                    spell: self.object(source)?,
                    target: self.target(target)?,
                    payment: plain_payment(allocation)?,
                },
            },
            PlayerAction::ActivateWithCreatures {
                source,
                ability,
                creatures,
            } => {
                if let Some(command) = self.granted_activation(
                    source,
                    ability,
                    spellwake_engine::activated::ActivationOrigin::Contributors(
                        self.objects(creatures)?,
                    ),
                    None,
                    None,
                )? {
                    return Ok(command);
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::ActivateWithCreatures {
                    source,
                    ability,
                    creatures: creatures
                        .iter()
                        .map(|handle| self.object(handle))
                        .collect::<Result<Vec<_>, _>>()?,
                }
            }
            PlayerAction::ActivateWithCostObjects {
                source,
                ability,
                target,
                objects,
                payment: allocation,
            } => {
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_ability(
                        source,
                        ability,
                        target.as_ref(),
                        Some(objects),
                        None,
                        allocation,
                    );
                }
                let source_id = self.object(source)?;
                let (ability_id, granting_effect) = match self.reference(ability)? {
                    NativeReference::GrantedAbility {
                        source: bound,
                        effect,
                    } if bound == source_id => (
                        spellwake_engine::activated::ActivatedAbilityId(0),
                        Some(effect),
                    ),
                    NativeReference::ActivatedAbility {
                        source: bound,
                        ability,
                    } if bound == source_id => (ability, None),
                    _ => return Err(NativeBridgeError::WrongReferenceKind),
                };
                GameCommand::ActivateWithCostObjects {
                    source: source_id,
                    ability: ability_id,
                    granting_effect,
                    target: target
                        .as_ref()
                        .map(|value| self.target(value))
                        .transpose()?,
                    objects: self.objects(objects)?,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::ActivateUntargetedAbility {
                source,
                ability,
                payment: allocation,
            } => {
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_ability(source, ability, None, None, None, allocation);
                }
                if let NativeReference::GrantedAbility {
                    source: bound,
                    effect,
                } = self.reference(ability)?
                {
                    let source = self.object(source)?;
                    if bound != source {
                        return Err(NativeBridgeError::InvalidReference);
                    }
                    return Ok(GameCommand::ActivateGranted {
                        source,
                        effect,
                        target: None,
                        payment: allocation.as_ref().map(plain_payment).transpose()?,
                    });
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::ActivateUntargetedAbility {
                    source,
                    ability,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::Cycle {
                source,
                ability,
                payment: allocation,
            } => {
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_special_ability(
                        source,
                        ability,
                        spellwake_engine::activated::ActivationOrigin::Cycling,
                        allocation,
                    );
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::Cycle {
                    source,
                    ability,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::ActivateEmblem {
                emblem_index,
                creature,
                payment,
            } => {
                let actor = self.seat(self.viewer_seat_id())?;
                let emblem = self
                    .source
                    .execution()
                    .emblems()
                    .filter(|value| value.controller == actor)
                    .nth(*emblem_index as usize)
                    .ok_or(NativeBridgeError::InvalidIntent)?;
                GameCommand::ActivateEmblem {
                    emblem: emblem.id,
                    creature: self.object(creature)?,
                    payment: payment.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::ActivatePermanentTargets {
                source,
                ability,
                targets,
                payment: allocation,
            } => {
                let announced = targets
                    .iter()
                    .map(|target| self.target(target))
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(command) = self.granted_activation(
                    source,
                    ability,
                    spellwake_engine::activated::ActivationOrigin::PermanentTargets(
                        announced.clone(),
                    ),
                    None,
                    allocation.as_ref(),
                )? {
                    return Ok(command);
                }
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_special_ability(
                        source,
                        ability,
                        spellwake_engine::activated::ActivationOrigin::PermanentTargets(announced),
                        allocation,
                    );
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::ActivatePermanentTargets {
                    source,
                    ability,
                    targets: announced,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::Ninjutsu {
                source,
                ability,
                attacker,
                payment: allocation,
            } => {
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_special_ability(
                        source,
                        ability,
                        spellwake_engine::activated::ActivationOrigin::Ninjutsu(
                            self.object(attacker)?,
                        ),
                        allocation,
                    );
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::Ninjutsu {
                    source,
                    ability,
                    attacker: self.object(attacker)?,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::Transmute {
                source,
                ability,
                payment: allocation,
            } => {
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_special_ability(
                        source,
                        ability,
                        spellwake_engine::activated::ActivationOrigin::Transmute,
                        allocation,
                    );
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::Transmute {
                    source,
                    ability,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::ActivateUntargetedX {
                source,
                ability,
                chosen_x,
                payment: allocation,
            } => {
                if let Some(command) = self.granted_activation(
                    source,
                    ability,
                    spellwake_engine::activated::ActivationOrigin::Untargeted,
                    Some(chosen_x.as_rules_integer().clone()),
                    allocation.as_ref(),
                )? {
                    return Ok(command);
                }
                if let Some(allocation) = allocation {
                    return self.restricted_ability(
                        source,
                        ability,
                        None,
                        None,
                        Some(chosen_x.as_rules_integer().clone()),
                        allocation,
                    );
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::ActivateWithRestrictedMana {
                    source,
                    ability,
                    granting_effect: None,
                    origin: spellwake_engine::activated::ActivationOrigin::Untargeted,
                    chosen_x: Some(chosen_x.as_rules_integer().clone()),
                    payment: None,
                    restricted_mana: Vec::new(),
                }
            }
            PlayerAction::ActivateLoyaltyX {
                source,
                ability,
                target,
                chosen_x,
                payment: allocation,
            } => {
                if let Some(command) = self.granted_activation(
                    source,
                    ability,
                    spellwake_engine::activated::ActivationOrigin::Targeted(self.target(target)?),
                    Some(chosen_x.as_rules_integer().clone()),
                    allocation.as_ref(),
                )? {
                    return Ok(command);
                }
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_ability(
                        source,
                        ability,
                        Some(target),
                        None,
                        Some(chosen_x.as_rules_integer().clone()),
                        allocation,
                    );
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::ActivateLoyaltyX {
                    source,
                    ability,
                    target: self.target(target)?,
                    chosen_x: chosen_x.as_rules_integer().clone(),
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::ActivateAbility {
                source,
                ability,
                target,
                payment: allocation,
            } => {
                if let Some(allocation) = allocation
                    .as_ref()
                    .filter(|value| !value.restricted_mana.is_empty())
                {
                    return self.restricted_ability(
                        source,
                        ability,
                        Some(target),
                        None,
                        None,
                        allocation,
                    );
                }
                if let NativeReference::GrantedAbility {
                    source: bound,
                    effect,
                } = self.reference(ability)?
                {
                    let source = self.object(source)?;
                    if bound != source {
                        return Err(NativeBridgeError::InvalidReference);
                    }
                    return Ok(GameCommand::ActivateGranted {
                        source,
                        effect,
                        target: Some(self.target(target)?),
                        payment: allocation.as_ref().map(plain_payment).transpose()?,
                    });
                }
                let (source, ability) = self.activated_source(source, ability)?;
                GameCommand::ActivateAbility {
                    source,
                    ability,
                    target: self.target(target)?,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::ActivateModalAbility {
                source,
                ability,
                modes,
                payment: allocation,
            } => {
                let (source, ability) = self.activated_source(source, ability)?;
                let mut cast_modes = Vec::with_capacity(modes.len());
                for m in modes {
                    cast_modes.push(spellwake_engine::mana::CastMode {
                        mode: m.mode,
                        target: m.target.as_ref().map(|t| self.target(t)).transpose()?,
                        targets: m
                            .targets
                            .iter()
                            .map(|t| self.target(t))
                            .collect::<Result<_, _>>()?,
                    });
                }
                GameCommand::ActivateModalAbility {
                    source,
                    ability,
                    modes: cast_modes,
                    payment: allocation.as_ref().map(plain_payment).transpose()?,
                }
            }
            PlayerAction::ActivateMana { source, ability } => {
                // ResolutionPayment expressly permits this action, including a
                // nested color decision. Do not add a blanket decision gate.
                let source = self.object(source)?;
                let NativeReference::ManaAbility {
                    source: bound_source,
                    ability,
                } = self.reference(ability)?
                else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                if source != bound_source {
                    return Err(NativeBridgeError::InvalidReference);
                }
                GameCommand::ActivateMana { source, ability }
            }
            PlayerAction::ActivateManaForPlayer {
                source,
                ability,
                recipient_seat_id,
            } => {
                let (source, ability) = self.mana_ability(source, ability)?;
                GameCommand::ActivateManaForPlayer {
                    source,
                    ability,
                    recipient: self.seat(*recipient_seat_id)?,
                }
            }
            PlayerAction::ActivateManaWithCostObjects {
                source,
                ability,
                objects,
            } => {
                let (source, ability) = self.mana_ability(source, ability)?;
                GameCommand::ActivateManaWithCostObjects {
                    source,
                    ability,
                    objects: self.objects(objects)?,
                }
            }
            PlayerAction::ActivateManaWithPayment {
                source,
                ability,
                payment: price,
            } => {
                let (source, ability) = self.mana_ability(source, ability)?;
                if price.exile_cast_type.is_some()
                    || price.graveyard_permission.is_some()
                    || price.alternative_cost_source.is_some()
                {
                    return Err(NativeBridgeError::InvalidIntent);
                }
                if !price.restricted_mana.is_empty() {
                    return Ok(GameCommand::ActivateManaWithRestrictedPayment {
                        source,
                        ability,
                        payment: payment(price),
                        restricted_mana: restricted_payment(
                            price,
                            self.source.execution().creature_vocabulary(),
                        )?,
                        bundle: None,
                    });
                }
                GameCommand::ActivateManaWithPayment {
                    source,
                    ability,
                    payment: plain_payment(price)?,
                }
            }
            PlayerAction::ActivateHybridMana {
                source,
                ability,
                payment: price,
                bundle: output,
            } => {
                let (source, ability) = self.mana_ability(source, ability)?;
                if price.exile_cast_type.is_some()
                    || price.graveyard_permission.is_some()
                    || price.alternative_cost_source.is_some()
                {
                    return Err(NativeBridgeError::InvalidIntent);
                }
                if !price.restricted_mana.is_empty() {
                    return Ok(GameCommand::ActivateManaWithRestrictedPayment {
                        source,
                        ability,
                        payment: payment(price),
                        restricted_mana: restricted_payment(
                            price,
                            self.source.execution().creature_vocabulary(),
                        )?,
                        bundle: Some(bundle(output)?),
                    });
                }
                GameCommand::ActivateHybridMana {
                    source,
                    ability,
                    payment: plain_payment(price)?,
                    bundle: bundle(output)?,
                }
            }
            PlayerAction::ActivateManaWithBundle {
                source,
                ability,
                bundle: output,
            } => {
                let (source, ability) = self.mana_ability(source, ability)?;
                GameCommand::ActivateManaWithBundle {
                    source,
                    ability,
                    bundle: bundle(output)?,
                }
            }
            PlayerAction::PlayLand { source } => GameCommand::PlayLand {
                land: self.object(source)?,
            },
            PlayerAction::PlayLandWithEntryColor { source, color } => {
                GameCommand::PlayLandWithEntryColor {
                    land: self.object(source)?,
                    color: native_mana_color(*color),
                }
            }
            PlayerAction::PlayLandWithEntryReveal { source, reveal } => {
                GameCommand::PlayLandWithEntryReveal {
                    land: self.object(source)?,
                    reveal: reveal
                        .as_ref()
                        .map(|value| self.object(value))
                        .transpose()?,
                }
            }
            PlayerAction::Pass {} => GameCommand::Pass,
            PlayerAction::Choose { decision, response } => {
                let NativeReference::Decision(id) = self.reference(decision)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                let actual = self
                    .source
                    .pending_decision()
                    .ok_or(NativeBridgeError::WrongDecision)?;
                if actual.id() != id {
                    return Err(NativeBridgeError::WrongDecision);
                }
                let choice = self.translate_reply(self.frame()?, response)?;
                GameCommand::Reply {
                    decision: id,
                    choice,
                }
            }
            PlayerAction::Concede {} => GameCommand::Concede,
        })
    }
    fn cast_payment(
        &self,
        use_alternative: bool,
        allocation: Option<&PlayerPayment>,
        resources: &[crate::player_wire::PlayerCastResource],
        additional: &[crate::player_wire::PlayerAdditionalCost],
    ) -> Result<spellwake_engine::mana::CastingPayment, NativeBridgeError> {
        use crate::player_wire::PlayerCastResource;
        use spellwake_engine::{
            characteristics::Color,
            mana::{CastResource, CastingPayment},
        };
        let mut splice = Vec::new();
        let mut objects = Vec::new();
        for resource in resources {
            let id = self.object(resource.source())?;
            objects.push(match resource {
                PlayerCastResource::Splice { target, .. } => {
                    splice.push(spellwake_engine::mana::SpliceChoice {
                        card: id,
                        target: target.as_ref().map(|t| self.target(t)).transpose()?,
                    });
                    continue;
                }
                PlayerCastResource::WebSlinging { .. } => CastResource::WebSlinging(id),
                PlayerCastResource::Emerge { .. } => CastResource::Emerge(id),
                PlayerCastResource::Sneak { .. } => CastResource::Sneak(id),
                PlayerCastResource::Bargain { .. } => CastResource::Bargain(id),
                PlayerCastResource::Retrace { .. } => CastResource::Retrace(id),
                PlayerCastResource::TapCreatureForSpell { .. } => {
                    CastResource::TapCreatureForSpell(id)
                }
                PlayerCastResource::TapForFlashback { .. } => CastResource::TapForFlashback(id),
                PlayerCastResource::ExileHandForAlternative { .. } => {
                    CastResource::ExileHandForAlternative(id)
                }
                PlayerCastResource::TapForAlternative { .. } => CastResource::TapForAlternative(id),
                PlayerCastResource::RevealColorlessCreature { .. } => {
                    CastResource::RevealColorlessCreature(id)
                }
                PlayerCastResource::SacrificeForSpell { .. } => CastResource::SacrificeForSpell(id),
                PlayerCastResource::ExileGraveyardCardForSpell { .. } => {
                    CastResource::ExileGraveyardCardForSpell(id)
                }
                PlayerCastResource::ExileGraveyardCreatureForSpell { .. } => {
                    CastResource::ExileGraveyardCreatureForSpell(id)
                }
                PlayerCastResource::DiscardForSpell { .. } => CastResource::DiscardForSpell(id),
                PlayerCastResource::JumpStart { .. } => CastResource::JumpStart(id),
                PlayerCastResource::Escape { .. } => CastResource::Escape(id),
                PlayerCastResource::FlashbackSacrifice { .. } => {
                    CastResource::FlashbackSacrifice(id)
                }
                PlayerCastResource::ExileCreatureForReduction { .. } => {
                    CastResource::ExileCreatureForReduction(id)
                }
                PlayerCastResource::Delve { .. } => CastResource::Delve(id),
                PlayerCastResource::ExtraCost { .. } => CastResource::ExtraCost(id),
                PlayerCastResource::Improvise { .. } => CastResource::Improvise(id),
                PlayerCastResource::Convoke { color, .. } => CastResource::Convoke {
                    creature: id,
                    color: match color {
                        None => None,
                        Some(ManaColor::White) => Some(Color::White),
                        Some(ManaColor::Blue) => Some(Color::Blue),
                        Some(ManaColor::Black) => Some(Color::Black),
                        Some(ManaColor::Red) => Some(Color::Red),
                        Some(ManaColor::Green) => Some(Color::Green),
                        Some(ManaColor::Colourless) => {
                            return Err(NativeBridgeError::InvalidReference);
                        }
                    },
                },
            });
        }
        Ok(CastingPayment {
            splice,
            exile_cast_type: allocation
                .and_then(|value| value.exile_cast_type)
                .map(native_card_type),
            alternative_cost_source: allocation
                .and_then(|value| value.alternative_cost_source.as_ref())
                .map(|value| self.object(value))
                .transpose()?,
            graveyard_permission: allocation
                .and_then(|value| value.graveyard_permission.as_ref())
                .map(|value| self.object(value))
                .transpose()?,
            hybrid_choices: Vec::new(),
            fixed_targets: Vec::new(),
            permanent_targets: Vec::new(),
            target_division: Vec::new(),
            chosen_x: None,
            modes: Vec::new(),
            adventure: false,
            split_half: None,
            disturb: false,
            modal_back: false,
            prototype: false,
            prepare: false,
            use_alternative,
            use_granted_flashback: allocation.is_some_and(|value| value.use_granted_flashback),
            mana: allocation.map(payment),
            restricted_mana: allocation
                .map(|value| {
                    restricted_payment(value, self.source.execution().creature_vocabulary())
                })
                .transpose()?
                .unwrap_or_default(),
            objects,
            additional: additional
                .iter()
                .map(|s| (usize::from(s.index), s.times.as_rules_integer().clone()))
                .collect(),
        })
    }
    fn objects(&self, handles: &[PlayerHandle]) -> Result<Vec<ObjectId>, NativeBridgeError> {
        handles.iter().map(|handle| self.object(handle)).collect()
    }
    fn activated_source(
        &self,
        source: &PlayerHandle,
        ability: &PlayerHandle,
    ) -> Result<(ObjectId, spellwake_engine::activated::ActivatedAbilityId), NativeBridgeError>
    {
        let source = self.object(source)?;
        let NativeReference::ActivatedAbility {
            source: bound_source,
            ability,
        } = self.reference(ability)?
        else {
            return Err(NativeBridgeError::WrongReferenceKind);
        };
        if source != bound_source {
            return Err(NativeBridgeError::InvalidReference);
        }
        Ok((source, ability))
    }
    fn entry_player_pair(
        &self,
        targets: &[PlayerTarget],
    ) -> Result<[SeatId; 2], NativeBridgeError> {
        let [first, second] = targets else {
            return Err(NativeBridgeError::InvalidIntent);
        };
        let (TargetRef::Player(first), TargetRef::Player(second)) =
            (self.target(first)?, self.target(second)?)
        else {
            return Err(NativeBridgeError::WrongReferenceKind);
        };
        Ok([first, second])
    }

    fn target(&self, target: &PlayerTarget) -> Result<TargetRef, NativeBridgeError> {
        match target {
            PlayerTarget::Object { handle } => Ok(TargetRef::Object(self.object(handle)?)),
            PlayerTarget::Stack { handle } => Ok(TargetRef::Stack(self.stack(handle)?)),
            PlayerTarget::Seat { seat_id } => Ok(TargetRef::Player(self.seat(*seat_id)?)),
        }
    }
    fn recipient(&self, target: &PlayerTarget) -> Result<DamageRecipient, NativeBridgeError> {
        match target {
            PlayerTarget::Seat { seat_id } => Ok(DamageRecipient::Player(self.seat(*seat_id)?)),
            PlayerTarget::Object { handle } => {
                let id = self.object(handle)?;
                self.source
                    .execution()
                    .current_characteristics(id)
                    .map_err(|cause| NativeBridgeError::Observation(Box::new(cause)))?
                    .and_then(|view| view.damage_recipient())
                    .ok_or(NativeBridgeError::Unsupported)
            }
            PlayerTarget::Stack { .. } => Err(NativeBridgeError::Unsupported),
        }
    }
    fn translate_reply(
        &self,
        frame: &RuleFrame,
        choice: &PlayerChoice,
    ) -> Result<DecisionReply, NativeBridgeError> {
        Ok(match (frame, choice) {
            (RuleFrame::Mutation(_), PlayerChoice::Number { value }) => {
                DecisionReply::MutateOnTop(match value.to_usize() {
                    Some(1) => true,
                    Some(2) => false,
                    _ => return Err(NativeBridgeError::WrongReferenceKind),
                })
            }
            (RuleFrame::LibraryTop(frame), PlayerChoice::Number { value })
                if frame.chooses_land_kind() =>
            {
                DecisionReply::ChooseLandKind(match value.to_usize() {
                    Some(1) => true,
                    Some(2) => false,
                    _ => return Err(NativeBridgeError::WrongReferenceKind),
                })
            }
            (RuleFrame::LibraryTop(frame), PlayerChoice::Scry { top, bottom })
                if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Scry =>
            {
                DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Scry {
                    top_first: self.objects(top)?,
                    bottom_top_first: self.objects(bottom)?,
                })
            }
            (RuleFrame::LibraryTop(frame), PlayerChoice::Surveil { top, graveyard })
                if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Surveil =>
            {
                DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Surveil {
                    top_first: self.objects(top)?,
                    to_graveyard: self.objects(graveyard)?,
                })
            }
            (RuleFrame::LibraryTop(frame), PlayerChoice::Objects { objects })
                if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Search =>
            {
                let selected = self.objects(objects)?;
                if frame.is_bounded_batch_search() && selected.len() <= frame.search_batch_limit() {
                    DecisionReply::LibraryTop(
                        spellwake_engine::effects::LibraryTopReply::SearchWithEntryChoices {
                            selected,
                            choices: vec![],
                        },
                    )
                } else if frame.is_split_search() && selected.len() <= 2 {
                    DecisionReply::LibraryTop(
                        spellwake_engine::effects::LibraryTopReply::SearchSplit {
                            battlefield: selected.first().copied(),
                            hand: selected.get(1).copied(),
                        },
                    )
                } else if frame.is_battlefield_batch_search() && selected.len() <= 2 {
                    DecisionReply::LibraryTop(
                        spellwake_engine::effects::LibraryTopReply::SearchBattlefield {
                            selected_in_timestamp_order: selected,
                        },
                    )
                } else if !frame.is_split_search()
                    && !frame.is_battlefield_batch_search()
                    && selected.len() <= 1
                {
                    DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::Search {
                        selected: selected.first().copied(),
                    })
                } else {
                    return Err(NativeBridgeError::Unsupported);
                }
            }
            (
                RuleFrame::LibraryTop(frame),
                PlayerChoice::SearchHandGraveyard { hand, graveyard },
            ) if frame.is_hand_graveyard_search() => DecisionReply::LibraryTop(
                spellwake_engine::effects::LibraryTopReply::SearchHandGraveyard {
                    hand: hand.as_ref().map(|value| self.object(value)).transpose()?,
                    graveyard: graveyard
                        .as_ref()
                        .map(|value| self.object(value))
                        .transpose()?,
                },
            ),
            (
                RuleFrame::LibraryTop(frame),
                PlayerChoice::SearchToDestination {
                    object,
                    battlefield,
                },
            ) if frame.offers_search_destination() => {
                let selected = object
                    .as_ref()
                    .map(|value| self.object(value))
                    .transpose()?;
                let (battlefield, hand) = if *battlefield {
                    (selected, None)
                } else {
                    (None, selected)
                };
                DecisionReply::LibraryTop(spellwake_engine::effects::LibraryTopReply::SearchSplit {
                    battlefield,
                    hand,
                })
            }
            (
                RuleFrame::LibraryTop(frame),
                PlayerChoice::SearchWithEntryColors { objects, colors },
            ) if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Search => {
                DecisionReply::LibraryTop(
                    spellwake_engine::effects::LibraryTopReply::SearchWithEntryColors {
                        selected: self.objects(objects)?,
                        colors: colors
                            .iter()
                            .map(|entry| {
                                Ok((self.object(&entry.object)?, native_mana_color(entry.color)))
                            })
                            .collect::<Result<_, NativeBridgeError>>()?,
                    },
                )
            }
            (
                RuleFrame::LibraryTop(frame),
                PlayerChoice::SearchWithEntryChoices {
                    objects,
                    colors,
                    reveals,
                },
            ) if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Search => {
                let mut choices = colors
                    .iter()
                    .map(|entry| {
                        Ok((
                            self.object(&entry.object)?,
                            spellwake_engine::land::EntryChoice::Color(native_mana_color(
                                entry.color,
                            )),
                        ))
                    })
                    .collect::<Result<Vec<_>, NativeBridgeError>>()?;
                for entry in reveals {
                    choices.push((
                        self.object(&entry.object)?,
                        spellwake_engine::land::EntryChoice::Reveal(
                            entry
                                .card
                                .as_ref()
                                .map(|card| self.object(card))
                                .transpose()?,
                        ),
                    ));
                }
                DecisionReply::LibraryTop(
                    spellwake_engine::effects::LibraryTopReply::SearchWithEntryChoices {
                        selected: self.objects(objects)?,
                        choices,
                    },
                )
            }
            (RuleFrame::LibraryTop(_), _) => return Err(NativeBridgeError::Unsupported),
            (RuleFrame::ResolutionSelect(_), PlayerChoice::Objects { objects }) => {
                DecisionReply::SelectResolution(self.objects(objects)?)
            }
            (RuleFrame::CounterReplacement(frame), PlayerChoice::Number { value }) => {
                let count = String::from(value.clone())
                    .parse::<usize>()
                    .ok()
                    .filter(|count| *count <= frame.doublers())
                    .ok_or(NativeBridgeError::InvalidIntent)?;
                DecisionReply::ApplyCounterDoublersFirst(count)
            }
            (RuleFrame::TransformOrder(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::TransformOrder(self.objects(ordered)?)
            }
            (RuleFrame::UntapKeepTapped(_), PlayerChoice::Objects { objects }) => {
                DecisionReply::UntapKeepTapped(self.objects(objects)?)
            }
            (RuleFrame::LinkedReturnOrder(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::LinkedReturnOrder(self.objects(ordered)?)
            }
            (RuleFrame::ResolutionGraveyardOrder(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::OrderResolutionGraveyard(self.objects(ordered)?)
            }
            (
                RuleFrame::OptionalEffect(frame),
                PlayerChoice::ImpulseCast {
                    face,
                    adventure,
                    target,
                    payment,
                    resources,
                    additional,
                    mana_abilities,
                    hybrid_choices,
                    chosen_x,
                    modes,
                    use_alternative,
                    permanent_targets,
                    target_division,
                    fixed_targets,
                },
            ) if frame.exile_cast().is_some_and(|offer| {
                matches!(
                    offer.ability,
                    spellwake_engine::effects::ExileCastAbility::Miracle
                        | spellwake_engine::effects::ExileCastAbility::RevealedLibrary
                        | spellwake_engine::effects::ExileCastAbility::TargetedGraveyard
                        | spellwake_engine::effects::ExileCastAbility::Commander
                        | spellwake_engine::effects::ExileCastAbility::Siege
                        | spellwake_engine::effects::ExileCastAbility::FoundSpell
                        | spellwake_engine::effects::ExileCastAbility::MatchingHand
                        | spellwake_engine::effects::ExileCastAbility::Impulse
                )
            }) =>
            {
                DecisionReply::ImpulseCast {
                    mana_abilities: mana_abilities
                        .iter()
                        .map(|a| {
                            let source = self.object(&a.source)?;
                            let NativeReference::ManaAbility {
                                source: bound,
                                ability,
                            } = self.reference(&a.ability)?
                            else {
                                return Err(NativeBridgeError::WrongReferenceKind);
                            };
                            if bound != source {
                                return Err(NativeBridgeError::InvalidReference);
                            }
                            Ok(spellwake_engine::mana::CastManaActivation {
                                source,
                                ability,
                                payment: a
                                    .payment
                                    .as_ref()
                                    .map(|value| {
                                        if value.exile_cast_type.is_some()
                                            || value.graveyard_permission.is_some()
                                            || value.alternative_cost_source.is_some()
                                        {
                                            Err(NativeBridgeError::InvalidIntent)
                                        } else {
                                            Ok(self::payment(value))
                                        }
                                    })
                                    .transpose()?,
                                restricted_mana: a
                                    .payment
                                    .as_ref()
                                    .map(|value| {
                                        restricted_payment(
                                            value,
                                            self.source.execution().creature_vocabulary(),
                                        )
                                    })
                                    .transpose()?
                                    .unwrap_or_default(),
                                bundle: a.bundle.as_ref().map(bundle).transpose()?,
                                color: a.color.map(native_mana_color),
                                cost_objects: self.objects(&a.cost_objects)?,
                                recipient: a
                                    .recipient_seat_id
                                    .map(|seat| self.seat(seat))
                                    .transpose()?,
                            })
                        })
                        .collect::<Result<_, _>>()?,
                    target: target.as_ref().map(|t| self.target(t)).transpose()?,
                    payment: {
                        let mut result = self.cast_payment(
                            *use_alternative,
                            payment.as_ref(),
                            resources,
                            additional,
                        )?;
                        result.adventure = *adventure;
                        apply_cast_face(&mut result, *face);
                        result.hybrid_choices = hybrid_choices.iter().map(hybrid_side).collect();
                        result.fixed_targets = fixed_targets
                            .iter()
                            .map(|target| self.target(target))
                            .collect::<Result<_, _>>()?;
                        result.permanent_targets = permanent_targets
                            .iter()
                            .map(|target| self.target(target))
                            .collect::<Result<_, _>>()?;
                        result.target_division = target_division.clone();
                        result.chosen_x = chosen_x.as_ref().map(|x| x.as_rules_integer().clone());
                        result.modes = modes
                            .iter()
                            .map(|m| {
                                Ok(spellwake_engine::mana::CastMode {
                                    mode: m.mode,
                                    targets: m
                                        .targets
                                        .iter()
                                        .map(|t| self.target(t))
                                        .collect::<Result<_, _>>()?,
                                    target: m
                                        .target
                                        .as_ref()
                                        .map(|t| self.target(t))
                                        .transpose()?,
                                })
                            })
                            .collect::<Result<_, NativeBridgeError>>()?;
                        result
                    },
                }
            }
            (
                RuleFrame::OptionalEffect(frame),
                PlayerChoice::MadnessCast {
                    face,
                    adventure,
                    chosen_x,
                    target,
                    payment,
                    resources,
                    additional,
                    mana_abilities,
                },
            ) if frame.exile_cast().is_some_and(|offer| {
                offer.ability == spellwake_engine::effects::ExileCastAbility::Madness
            }) =>
            {
                DecisionReply::MadnessCast {
                    mana_abilities: mana_abilities
                        .iter()
                        .map(|a| {
                            let source = self.object(&a.source)?;
                            let NativeReference::ManaAbility {
                                source: bound,
                                ability,
                            } = self.reference(&a.ability)?
                            else {
                                return Err(NativeBridgeError::WrongReferenceKind);
                            };
                            if bound != source {
                                return Err(NativeBridgeError::InvalidReference);
                            }
                            Ok(spellwake_engine::mana::CastManaActivation {
                                source,
                                ability,
                                payment: a
                                    .payment
                                    .as_ref()
                                    .map(|value| {
                                        if value.exile_cast_type.is_some()
                                            || value.graveyard_permission.is_some()
                                            || value.alternative_cost_source.is_some()
                                        {
                                            Err(NativeBridgeError::InvalidIntent)
                                        } else {
                                            Ok(self::payment(value))
                                        }
                                    })
                                    .transpose()?,
                                restricted_mana: a
                                    .payment
                                    .as_ref()
                                    .map(|value| {
                                        restricted_payment(
                                            value,
                                            self.source.execution().creature_vocabulary(),
                                        )
                                    })
                                    .transpose()?
                                    .unwrap_or_default(),
                                bundle: a.bundle.as_ref().map(bundle).transpose()?,
                                color: a.color.map(native_mana_color),
                                cost_objects: self.objects(&a.cost_objects)?,
                                recipient: a
                                    .recipient_seat_id
                                    .map(|seat| self.seat(seat))
                                    .transpose()?,
                            })
                        })
                        .collect::<Result<_, _>>()?,
                    target: target.as_ref().map(|t| self.target(t)).transpose()?,
                    payment: {
                        let mut price =
                            self.cast_payment(false, payment.as_ref(), resources, additional)?;
                        price.adventure = *adventure;
                        apply_cast_face(&mut price, *face);
                        price.chosen_x = chosen_x.as_ref().map(|x| x.as_rules_integer().clone());
                        price
                    },
                }
            }
            (
                RuleFrame::OptionalEffect(frame),
                PlayerChoice::CascadeCast {
                    face,
                    adventure,
                    chosen_x,
                    target,
                    payment,
                    resources,
                    additional,
                },
            ) if frame.exile_cast().is_some_and(|offer| {
                offer.ability == spellwake_engine::effects::ExileCastAbility::Cascade
            }) =>
            {
                DecisionReply::CascadeCast {
                    target: target.as_ref().map(|t| self.target(t)).transpose()?,
                    payment: {
                        let mut price =
                            self.cast_payment(false, payment.as_ref(), resources, additional)?;
                        price.adventure = *adventure;
                        apply_cast_face(&mut price, *face);
                        price.chosen_x = chosen_x.as_ref().map(|x| x.as_rules_integer().clone());
                        price
                    },
                }
            }
            (
                RuleFrame::OptionalEffect(frame),
                PlayerChoice::ReboundCast {
                    face,
                    adventure,
                    chosen_x,
                    target,
                    payment,
                    resources,
                    additional,
                },
            ) if frame.exile_cast().is_some_and(|offer| {
                offer.ability == spellwake_engine::effects::ExileCastAbility::Rebound
            }) =>
            {
                DecisionReply::ReboundCast {
                    target: target.as_ref().map(|t| self.target(t)).transpose()?,
                    payment: {
                        let mut price =
                            self.cast_payment(false, payment.as_ref(), resources, additional)?;
                        price.adventure = *adventure;
                        apply_cast_face(&mut price, *face);
                        price.chosen_x = chosen_x.as_ref().map(|x| x.as_rules_integer().clone());
                        price
                    },
                }
            }
            (
                RuleFrame::OptionalEffect(frame),
                PlayerChoice::SuspendCast {
                    face,
                    adventure,
                    chosen_x,
                    target,
                    payment,
                    resources,
                    additional,
                },
            ) if frame.suspended_cast().is_some() => DecisionReply::SuspendCast {
                target: target.as_ref().map(|t| self.target(t)).transpose()?,
                payment: {
                    let mut price =
                        self.cast_payment(false, payment.as_ref(), resources, additional)?;
                    price.adventure = *adventure;
                    apply_cast_face(&mut price, *face);
                    price.chosen_x = chosen_x.as_ref().map(|x| x.as_rules_integer().clone());
                    price
                },
            },
            (RuleFrame::OptionalEffect(frame), PlayerChoice::Decline {})
                if frame.exile_cast().is_some() =>
            {
                DecisionReply::OptionalEffect(
                    spellwake_engine::effects::OptionalEffectChoice::Decline,
                )
            }
            (RuleFrame::ZoneReplacement(frame), PlayerChoice::Option { option }) => {
                let NativeReference::ZoneReplacement(index) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::ZoneReplacement(
                    frame
                        .reply_at(index)
                        .ok_or(NativeBridgeError::InvalidReference)?,
                )
            }
            (RuleFrame::OptionalEffect(_), PlayerChoice::Option { option }) => {
                let NativeReference::OptionalEffect(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::OptionalEffect(value)
            }
            (
                RuleFrame::ResolutionPayment(_),
                PlayerChoice::Pay {
                    payment: allocation,
                },
            ) => DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(plain_payment(
                allocation,
            )?)),
            (RuleFrame::ResolutionPayment(_), PlayerChoice::Decline {}) => {
                DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
            }
            (RuleFrame::TimestampOrder(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::TimestampOrder(
                    ordered
                        .iter()
                        .map(|handle| match self.reference(handle)? {
                            NativeReference::TokenMember(value) => Ok(value),
                            _ => Err(NativeBridgeError::WrongReferenceKind),
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
            (
                RuleFrame::Checks(
                    CheckDecision::CleanupDiscard(_) | CheckDecision::CleanupSelect(_),
                ),
                PlayerChoice::Objects { objects },
            ) => DecisionReply::Discard(self.objects(objects)?),
            (
                RuleFrame::Checks(CheckDecision::ReturnCommanders(_)),
                PlayerChoice::Objects { objects },
            ) => DecisionReply::ReturnCommanders(self.objects(objects)?),
            (
                RuleFrame::Checks(CheckDecision::GraveyardOrder(_)),
                PlayerChoice::Ordering { ordered },
            ) => DecisionReply::OrderGraveyard(self.objects(ordered)?),
            (
                RuleFrame::Checks(CheckDecision::Legend(_)),
                PlayerChoice::Legend { conflict, keep },
            ) => {
                let NativeReference::Legend(conflict) = self.reference(conflict)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::KeepLegend {
                    conflict,
                    keep: self.object(keep)?,
                }
            }
            (RuleFrame::Triggers(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::OrderTriggers(
                    ordered
                        .iter()
                        .map(|handle| match self.reference(handle)? {
                            NativeReference::Trigger(value) => Ok(value),
                            _ => Err(NativeBridgeError::WrongReferenceKind),
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
            (RuleFrame::RevealedPiles(frame), PlayerChoice::Objects { objects })
                if matches!(
                    frame.choice(),
                    spellwake_engine::effects::PileChoice::OneToHand(_)
                        | spellwake_engine::effects::PileChoice::OneToHandRestBottom(_)
                        | spellwake_engine::effects::PileChoice::OptionalCreatureToBattlefield { .. }
                        | spellwake_engine::effects::PileChoice::OptionalUpTo { .. }
                        | spellwake_engine::effects::PileChoice::Creature(_)
                        | spellwake_engine::effects::PileChoice::Divide(_)
                        | spellwake_engine::effects::PileChoice::DividePermanents(_)
                ) =>
            {
                DecisionReply::SelectResolution(self.objects(objects)?)
            }
            (RuleFrame::RevealedPiles(frame), PlayerChoice::Number { value })
                if matches!(
                    frame.choice(),
                    spellwake_engine::effects::PileChoice::Take { .. }
                        | spellwake_engine::effects::PileChoice::Sacrifice { .. }
                ) =>
            {
                DecisionReply::ChoosePile(
                    value
                        .to_usize()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or(NativeBridgeError::WrongReferenceKind)?,
                )
            }
            (RuleFrame::RevealedPiles(frame), PlayerChoice::Ordering { ordered })
                if matches!(
                    frame.choice(),
                    spellwake_engine::effects::PileChoice::BottomOrder(_)
                        | spellwake_engine::effects::PileChoice::GraveyardOrder(_)
                ) =>
            {
                DecisionReply::OrderResolutionGraveyard(self.objects(ordered)?)
            }
            (RuleFrame::PlaneswalkerType(_), PlayerChoice::Option { option }) => {
                let NativeReference::PlaneswalkerType(kind) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::PlaneswalkerType(kind)
            }
            (RuleFrame::AttackDirection(_), PlayerChoice::Option { option }) => {
                let NativeReference::AttackDirection(direction) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::AttackDirection(direction)
            }
            (RuleFrame::DrawReplacement(_), PlayerChoice::Option { option }) => {
                use spellwake_engine::effects::DrawReplacementChoice;
                DecisionReply::DrawReplacement(match self.reference(option)? {
                    NativeReference::MiracleReveal(reveal) => {
                        DrawReplacementChoice::Miracle(reveal)
                    }
                    NativeReference::DrawLandKind(None) => DrawReplacementChoice::Draw,
                    NativeReference::DrawLandKind(Some(true)) => DrawReplacementChoice::Land,
                    NativeReference::DrawLandKind(Some(false)) => DrawReplacementChoice::Nonland,
                    _ => return Err(NativeBridgeError::WrongReferenceKind),
                })
            }
            (RuleFrame::DrawReplacement(_), PlayerChoice::Target { target }) => {
                let TargetRef::Player(seat) = self.target(target)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::DrawReplacement(
                    spellwake_engine::effects::DrawReplacementChoice::Collector(seat),
                )
            }
            (RuleFrame::DrawReplacement(_), PlayerChoice::Ordering { ordered }) => {
                DecisionReply::DrawReplacement(
                    spellwake_engine::effects::DrawReplacementChoice::Bottom(
                        self.objects(ordered)?,
                    ),
                )
            }
            (RuleFrame::DamageRecipientChoice(_), PlayerChoice::Target { target }) => {
                DecisionReply::ChooseDamageRecipient(self.target(target)?)
            }
            (RuleFrame::OpponentChoice(_), PlayerChoice::Target { target }) => {
                let TargetRef::Player(seat) = self.target(target)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::OpponentChoice(seat)
            }
            (RuleFrame::ReplacementChooser(_), PlayerChoice::Target { target }) => {
                let TargetRef::Player(seat) = self.target(target)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::ReplacementChooser(seat)
            }
            (RuleFrame::MyriadDefender(_), PlayerChoice::TargetList { targets })
                if targets.len() <= 1 =>
            {
                DecisionReply::MyriadDefender(
                    targets
                        .first()
                        .map(|target| self.target(target))
                        .transpose()?,
                )
            }
            (RuleFrame::ReselectAttackDefender(_), PlayerChoice::TargetList { targets })
                if targets.len() <= 1 =>
            {
                DecisionReply::ReselectAttackDefender(
                    targets
                        .first()
                        .map(|target| self.target(target))
                        .transpose()?,
                )
            }
            (RuleFrame::TokenDefender(_), PlayerChoice::Target { target }) => {
                DecisionReply::TokenDefender(self.target(target)?)
            }
            (RuleFrame::TriggerTarget(frame), PlayerChoice::TriggerModes { modes })
                if frame.modal().is_some() =>
            {
                DecisionReply::TriggerModes(
                    modes
                        .iter()
                        .map(|mode| {
                            Ok(spellwake_engine::mana::CastMode {
                                mode: mode.mode,
                                targets: mode
                                    .targets
                                    .iter()
                                    .map(|target| self.target(target))
                                    .collect::<Result<_, _>>()?,
                                target: mode
                                    .target
                                    .as_ref()
                                    .map(|target| self.target(target))
                                    .transpose()?,
                            })
                        })
                        .collect::<Result<_, NativeBridgeError>>()?,
                )
            }
            (RuleFrame::TriggerTarget(frame), PlayerChoice::TargetList { targets })
                if frame.ordered_any_target_bounds().is_some() =>
            {
                DecisionReply::TriggerPermanentTargets(
                    targets
                        .iter()
                        .map(|target| self.target(target))
                        .collect::<Result<_, _>>()?,
                )
            }
            (RuleFrame::TriggerTarget(frame), PlayerChoice::Objects { objects })
                if frame.permanent_target_bounds().is_some() =>
            {
                DecisionReply::TriggerPermanentTargets(
                    self.objects(objects)?
                        .into_iter()
                        .map(TargetRef::Object)
                        .collect(),
                )
            }
            (RuleFrame::TriggerTarget(frame), PlayerChoice::Target { target })
                if frame.modal().is_none() =>
            {
                DecisionReply::TriggerTarget(self.target(target)?)
            }
            (RuleFrame::TokenAuraChoice(_), PlayerChoice::Target { target }) => {
                DecisionReply::TokenAuraChoice(self.target(target)?)
            }
            (RuleFrame::LinkedAuraChoice(_), PlayerChoice::Target { target }) => {
                DecisionReply::LinkedAuraChoice(self.target(target)?)
            }
            (
                RuleFrame::Combat(CombatDecision::Attackers(frame)),
                PlayerChoice::PayAttackCost {
                    life_symbols,
                    payment,
                },
            ) if frame.payment().is_some() => DecisionReply::PayAttackCost {
                life_symbols: life_symbols.as_rules_integer().clone(),
                mana: plain_payment(payment)?,
            },
            (
                RuleFrame::Combat(CombatDecision::Attackers(frame)),
                PlayerChoice::RedeclareReversingMana { activations },
            ) if frame.payment().is_some() => DecisionReply::RedeclareReversingMana(
                activations
                    .iter()
                    .map(|id| spellwake_engine::game::AttackManaId(*id))
                    .collect(),
            ),
            (
                RuleFrame::Combat(CombatDecision::Attackers(frame)),
                PlayerChoice::RedeclareKeepingMana {},
            ) if frame.payment().is_some() => DecisionReply::RedeclareKeepingMana,
            (RuleFrame::Combat(CombatDecision::Attackers(frame)), PlayerChoice::Decline {})
                if frame.payment().is_some() =>
            {
                DecisionReply::RedeclareAttackers
            }
            (RuleFrame::CastRecovery(_), PlayerChoice::Pay { payment: value }) => {
                DecisionReply::CastRecovery(
                    spellwake_engine::effects::CastRecoveryChoice::RetryRestricted {
                        payment: Some(payment(value)),
                        restricted_mana: restricted_payment(
                            value,
                            self.source.execution().creature_vocabulary(),
                        )?,
                    },
                )
            }
            (RuleFrame::CastRecovery(_), PlayerChoice::Decline {}) => {
                DecisionReply::CastRecovery(spellwake_engine::effects::CastRecoveryChoice::Abandon)
            }
            (
                RuleFrame::Combat(CombatDecision::Attackers(_)),
                PlayerChoice::Attackers { assignments },
            ) => {
                let mut bands = std::collections::BTreeMap::<u32, Vec<ObjectId>>::new();
                let attackers = assignments
                    .iter()
                    .map(|entry| {
                        let (defender, defending_planeswalker) = match &entry.defender {
                            PlayerTarget::Seat { seat_id } => (self.seat(*seat_id)?, None),
                            PlayerTarget::Object { handle } => {
                                let id = self.object(handle)?;
                                let view = self
                                    .source
                                    .execution()
                                    .current_characteristics(id)
                                    .map_err(|cause| {
                                        NativeBridgeError::Observation(Box::new(cause))
                                    })?
                                    .ok_or(NativeBridgeError::Unsupported)?;
                                (
                                    self.source
                                        .execution()
                                        .permanent_defender(&view)
                                        .ok_or(NativeBridgeError::Unsupported)?,
                                    Some(id),
                                )
                            }
                            PlayerTarget::Stack { .. } => {
                                return Err(NativeBridgeError::Unsupported);
                            }
                        };
                        let attacker = self.object(&entry.attacker)?;
                        if let Some(band) = entry.band {
                            bands.entry(band).or_default().push(attacker);
                        }
                        Ok(AttackDeclaration {
                            attacker,
                            defender,
                            defending_planeswalker,
                        })
                    })
                    .collect::<Result<_, _>>()?;
                if bands.is_empty() {
                    DecisionReply::DeclareAttackers(attackers)
                } else {
                    DecisionReply::DeclareBandedAttackers {
                        attackers,
                        bands: bands.into_values().collect(),
                    }
                }
            }
            (
                RuleFrame::Combat(CombatDecision::Blockers(_)),
                PlayerChoice::Blockers { assignments },
            ) => {
                let mut declarations = Vec::new();
                for entry in assignments {
                    let blocker = self.object(&entry.blocker)?;
                    for attacker in &entry.attackers {
                        declarations.push(BlockDeclaration {
                            blocker,
                            attacker: self.object(attacker)?,
                        });
                    }
                }
                DecisionReply::DeclareBlockers(declarations)
            }
            (
                RuleFrame::Combat(CombatDecision::DamageAssignments(_)),
                PlayerChoice::CombatDamage { assignments },
            ) => DecisionReply::AssignCombatDamage(
                assignments
                    .iter()
                    .map(|entry| {
                        Ok(CombatDamageAssignment {
                            source: self.object(&entry.source)?,
                            target: self.recipient(&entry.target)?,
                            amount: entry.amount.as_rules_integer().clone(),
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
            (
                RuleFrame::Opening(OpeningDecision::Declare { .. }),
                PlayerChoice::Option { option },
            ) => {
                let NativeReference::Mulligan(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::Mulligan(value)
            }
            (
                RuleFrame::Opening(OpeningDecision::Bottom { .. }),
                PlayerChoice::Objects { objects },
            ) => DecisionReply::Bottom(self.objects(objects)?),
            (
                RuleFrame::Opening(OpeningDecision::PregameBattlefield { .. }),
                PlayerChoice::Objects { objects },
            ) => DecisionReply::OpeningBattlefield(self.objects(objects)?),
            (RuleFrame::CopyTargets(_), PlayerChoice::Option { option }) => {
                let NativeReference::CopyTargets(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::CopyTargets(value)
            }
            (RuleFrame::Damage(DamageDecision::Order(_)), PlayerChoice::Option { option }) => {
                let NativeReference::DamageApplication(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::DamageOrder(value)
            }
            (
                RuleFrame::Damage(DamageDecision::Allocation(_)),
                PlayerChoice::Prevention { allocations },
            ) => DecisionReply::AllocatePrevention(
                allocations
                    .iter()
                    .map(|entry| {
                        let NativeReference::DamageAttempt(attempt) =
                            self.reference(&entry.attempt)?
                        else {
                            return Err(NativeBridgeError::WrongReferenceKind);
                        };
                        Ok(DamageAllocation {
                            attempt,
                            amount: entry.amount.as_rules_integer().clone(),
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ),
            (RuleFrame::ManaColor(_), PlayerChoice::Option { option }) => {
                let NativeReference::ManaColor(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::ManaColor(value)
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Option { option })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::AlwaysChooseColorExcept(_)
                        | spellwake_engine::land::EntryTap::ChooseColor
                        | spellwake_engine::land::EntryTap::ChooseColorExcept(_)
                ) =>
            {
                let NativeReference::ManaColor(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::Color(value))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Option { option })
                if frame.rule().option_labels().is_some() =>
            {
                let NativeReference::OptionIndex(index) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::OptionIndex(index))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Option { option })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::UnlessPayLife(_)
                ) =>
            {
                let NativeReference::EntryPayLife(pay) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::PayLife(pay))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Decline {})
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::UnlessPayLife(_)
                ) =>
            {
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::PayLife(false))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Option { option })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::UnlessPayLife(_)
                ) =>
            {
                let NativeReference::EntryPayLife(pay) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::PayLife(pay))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Decline {})
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::UnlessPayLife(_)
                ) =>
            {
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::PayLife(false))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Objects { objects })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::UnlessRevealHandLandType(_, _)
                ) =>
            {
                let values = self.objects(objects)?;
                if values.len() > 1 {
                    return Err(NativeBridgeError::Unsupported);
                }
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::Reveal(
                    values.first().copied(),
                ))
            }
            (
                RuleFrame::Checks(CheckDecision::BattleProtector(_)),
                PlayerChoice::Target { target },
            ) => {
                let TargetRef::Player(seat) = self.target(target)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::OpponentChoice(seat)
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Target { target })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::SiegeProtector
                        | spellwake_engine::land::EntryTap::ChosenOpponentControls
                        | spellwake_engine::land::EntryTap::ChooseOpponent
                        | spellwake_engine::land::EntryTap::ChoosePlayer
                ) =>
            {
                let TargetRef::Player(seat) = self.target(target)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::Opponent(seat))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Target { target })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::SiegeProtector
                        | spellwake_engine::land::EntryTap::ChosenOpponentControls
                        | spellwake_engine::land::EntryTap::ChooseOpponent
                        | spellwake_engine::land::EntryTap::ChoosePlayer
                ) =>
            {
                let TargetRef::Player(seat) = self.target(target)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::Opponent(seat))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Option { option })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::AlwaysChooseColorExcept(_)
                        | spellwake_engine::land::EntryTap::ChooseColor
                        | spellwake_engine::land::EntryTap::ChooseColorExcept(_)
                ) =>
            {
                let NativeReference::ManaColor(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::Color(value))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Option { option })
                if frame.rule().option_labels().is_some() =>
            {
                let NativeReference::OptionIndex(index) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::OptionIndex(
                    index,
                ))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Objects { objects })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::UnlessRevealHandLandType(_, _)
                ) =>
            {
                let values = self.objects(objects)?;
                if values.len() > 1 {
                    return Err(NativeBridgeError::Unsupported);
                }
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::Reveal(
                    values.first().copied(),
                ))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::TargetList { targets })
                if frame.rule() == spellwake_engine::land::EntryTap::ChooseTwoPlayers =>
            {
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::Players(
                    self.entry_player_pair(targets)?,
                ))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::TargetList { targets })
                if frame.rule() == spellwake_engine::land::EntryTap::ChooseTwoPlayers =>
            {
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::Players(
                    self.entry_player_pair(targets)?,
                ))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Option { option })
                if frame.rule() == spellwake_engine::land::EntryTap::ChooseNonlandCardName =>
            {
                let NativeReference::CardName(index) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::CardName(index))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Option { option })
                if frame.rule() == spellwake_engine::land::EntryTap::ChooseNonlandCardName =>
            {
                let NativeReference::CardName(index) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::CardName(
                    index,
                ))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Option { option })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::ChooseCreatureType
                ) =>
            {
                let NativeReference::CreatureType(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::CreatureType(value))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Option { option })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::ChooseCreatureType
                ) =>
            {
                let NativeReference::CreatureType(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::CreatureType(
                    value,
                ))
            }
            (RuleFrame::EntryChoice(frame), PlayerChoice::Number { value })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::ReadAhead { .. }
                ) =>
            {
                DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::Chapter(
                    value
                        .to_usize()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or(NativeBridgeError::WrongReferenceKind)?,
                ))
            }
            (RuleFrame::TokenEntryChoice(frame), PlayerChoice::Number { value })
                if matches!(
                    frame.rule(),
                    spellwake_engine::land::EntryTap::ReadAhead { .. }
                ) =>
            {
                DecisionReply::TokenEntryChoice(spellwake_engine::land::EntryChoice::Chapter(
                    value
                        .to_usize()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or(NativeBridgeError::WrongReferenceKind)?,
                ))
            }
            (RuleFrame::CounterRemoval(_), PlayerChoice::CounterRemoval { counters }) => {
                DecisionReply::RemoveCounters(
                    counters
                        .iter()
                        .map(|entry| (entry.kind.clone().into(), entry.count))
                        .collect(),
                )
            }
            (RuleFrame::Proliferate(_), PlayerChoice::TargetList { targets }) => {
                DecisionReply::Proliferate(
                    targets
                        .iter()
                        .map(|who| self.target(who))
                        .collect::<Result<_, _>>()?,
                )
            }
            (RuleFrame::LoyaltyRemoval(_), PlayerChoice::Objects { objects }) => {
                DecisionReply::SelectResolution(self.objects(objects)?)
            }
            (RuleFrame::ResolutionManaColor(frame), PlayerChoice::Number { value })
                if frame.split().is_some() =>
            {
                DecisionReply::SplitMana(value.clone())
            }
            (RuleFrame::ResolutionManaColor(_), PlayerChoice::Option { option }) => {
                let NativeReference::ManaColor(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::ManaColor(value)
            }
            (RuleFrame::OptionIndex(_), PlayerChoice::Option { option }) => {
                let NativeReference::OptionIndex(index) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::OptionIndex(index)
            }
            (RuleFrame::CreatureType(_), PlayerChoice::Option { option }) => {
                let NativeReference::CreatureType(value) = self.reference(option)? else {
                    return Err(NativeBridgeError::WrongReferenceKind);
                };
                DecisionReply::CreatureType(value)
            }
            _ => return Err(NativeBridgeError::Unsupported),
        })
    }
}

#[cfg(test)]
mod face_announcement_tests {
    use super::*;
    use crate::player_wire::PlayerAction;
    use spellwake_engine::{mana::CastingPayment, program::SplitHalf};

    #[test]
    fn player_face_choice_reaches_the_native_cast_announcement() {
        for (label, split, disturb) in [
            (None, None, false),
            (Some("split-left"), Some(SplitHalf::Left), false),
            (Some("split-right"), Some(SplitHalf::Right), false),
            (Some("disturb"), None, true),
        ] {
            let mut json = serde_json::json!({"kind":"cast-with-resources","source":"physical","target":null,"payment":null,"resources":[]});
            if let Some(label) = label {
                json["face"] = label.into();
            }
            let action: PlayerAction = serde_json::from_value(json).unwrap();
            let PlayerAction::CastWithResources { face, .. } = action else {
                panic!("expected a cast announcement")
            };
            let mut payment = CastingPayment::default();
            apply_cast_face(&mut payment, face);
            assert_eq!((payment.split_half, payment.disturb), (split, disturb));
        }
    }
}
