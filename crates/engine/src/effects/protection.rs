use super::*;
use crate::{characteristics::Color, occurrence::SupportedAppearance, stack_window::StackEntry};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ProtectionSource {
    colors: BTreeSet<Color>,
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    pub card_types: BTreeSet<CardType>,
    pub is_creature: bool,
    pub controller: Option<SeatId>,
    pub is_ability: bool,
    pub is_aura: bool,
}
impl ProtectionSource {
    pub(super) fn with_controller(mut self, controller: SeatId) -> Self {
        self.controller = Some(controller);
        self
    }

    pub(super) fn with_ability_controller(mut self, controller: SeatId) -> Self {
        self.controller = Some(controller);
        self.is_ability = true;
        self
    }
}
impl From<&ObjectCharacteristics> for ProtectionSource {
    fn from(source: &ObjectCharacteristics) -> Self {
        Self {
            colors: source.current_colors.clone(),
            creature_subtypes: source.current_creature_subtypes.clone(),
            card_types: source.current_types.clone(),
            is_creature: source.is_creature(),
            controller: source.controller,
            is_ability: false,
            is_aura: matches!(
                source.attachment_kind,
                Some(crate::attachment::AttachmentKind::Aura(_))
            ),
        }
    }
}
impl From<&CopiableSpellTraits> for ProtectionSource {
    fn from(source: &CopiableSpellTraits) -> Self {
        Self {
            colors: source.colors.clone(),
            creature_subtypes: source.creature_subtypes.clone(),
            card_types: source.card_types.clone(),
            is_creature: source.card_types.contains(&CardType::Creature),
            controller: None,
            is_ability: false,
            is_aura: false,
        }
    }
}
impl From<&Program> for ProtectionSource {
    fn from(source: &Program) -> Self {
        Self {
            colors: if source.printed_keywords.contains(&KeywordAbility::Devoid) {
                BTreeSet::new()
            } else {
                source.defined_colors()
            },
            creature_subtypes: source.creature_subtypes.clone(),
            card_types: source.card_types.clone(),
            is_creature: source.has_type(CardType::Creature),
            controller: None,
            is_ability: false,
            is_aura: matches!(
                source.permanent().and_then(|p| p.attachment),
                Some(crate::attachment::AttachmentKind::Aura(_))
            ),
        }
    }
}

impl EffectWindow {
    /// Copiable colors stored with an instruction spell, including noncard copies.
    pub fn stack_copy_colors(&self, entry: StackEntryId) -> Option<&BTreeSet<Color>> {
        match self.payloads.get(&entry)? {
            StackPayload::Instructions(payload) => Some(&payload.copiable_traits.as_ref()?.colors),
            StackPayload::Permanent => None,
        }
    }
    pub(super) fn protected_target(
        target: TargetRef,
        source: &ProtectionSource,
        current: &EvaluatedCharacteristics,
    ) -> bool {
        match target {
            TargetRef::Object(id) => current.get(id).is_some_and(|object| {
                let hexproof_ignored = source
                    .controller
                    .is_some_and(|ctrl| object.hexproof_ignored_by.contains(&ctrl));
                let restricted_by_static =
                    (object.cannot_be_targeted_by_auras && source.is_aura && !source.is_ability)
                        || (!object.cannot_be_targeted_by_colors.is_empty()
                            && object
                                .cannot_be_targeted_by_colors
                                .matches_any(&source.colors))
                        || (!source.is_ability
                            && !object.cannot_be_targeted_by_spells_of_colors.is_empty()
                            && object
                                .cannot_be_targeted_by_spells_of_colors
                                .matches_any(&source.colors));
                object.zone == ObjectZone::Battlefield
                    && (restricted_by_static
                        || object.protected_from_source(
                            &source.colors,
                            &source.card_types,
                            &source.creature_subtypes,
                            source.is_creature,
                            source.controller,
                        )
                        || (!hexproof_ignored
                            && source.controller.is_some_and(|ctrl| {
                                object.controller.is_some_and(|owner| owner != ctrl)
                            })
                            && object.hexproof_from_source(
                                &source.colors,
                                &source.card_types,
                                &source.creature_subtypes,
                                source.is_ability,
                            )))
            }),
            TargetRef::Player(player) => {
                current
                    .player_protection_colors(player)
                    .any(|color| source.colors.contains(&color))
                    || current
                        .player_granted_keywords
                        .get(&player)
                        .is_some_and(|keywords| {
                            keywords.iter().any(|kw| match kw {
                                KeywordAbility::Shroud => true,
                                KeywordAbility::Hexproof => {
                                    source.controller.is_some_and(|ctrl| ctrl != player)
                                }
                                KeywordAbility::HexproofFrom(quality) => {
                                    source.controller.is_some_and(|ctrl| ctrl != player)
                                        && match quality {
                                            crate::characteristics::HexproofQuality::Color(c) => {
                                                source.colors.contains(c)
                                            }
                                            crate::characteristics::HexproofQuality::Monocolored => {
                                                source.colors.len() == 1
                                            }
                                            crate::characteristics::HexproofQuality::Multicolored => {
                                                source.colors.len() >= 2
                                            }
                                            crate::characteristics::HexproofQuality::CardType(ct) => {
                                                source.card_types.contains(ct)
                                            }
                                            crate::characteristics::HexproofQuality::Subtype(st) => {
                                                source.creature_subtypes.contains(st)
                                            }
                                            crate::characteristics::HexproofQuality::ActivatedOrTriggeredAbility => {
                                                source.is_ability
                                            }
                                        }
                                }
                                KeywordAbility::ProtectionFromWhite => {
                                    source.colors.contains(&Color::White)
                                }
                                KeywordAbility::ProtectionFromBlue => {
                                    source.colors.contains(&Color::Blue)
                                }
                                KeywordAbility::ProtectionFromBlack => {
                                    source.colors.contains(&Color::Black)
                                }
                                KeywordAbility::ProtectionFromRed => {
                                    source.colors.contains(&Color::Red)
                                }
                                KeywordAbility::ProtectionFromGreen => {
                                    source.colors.contains(&Color::Green)
                                }
                                KeywordAbility::ProtectionFromMulticolored => {
                                    source.colors.len() >= 2
                                }
                                KeywordAbility::ProtectionFromSubtype(sub) => {
                                    sub.matches(&source.creature_subtypes, source.is_creature)
                                }
                                KeywordAbility::ProtectionFrom(quality) => match quality {
                                    crate::characteristics::ProtectionFromQuality::Colorless => {
                                        source.colors.is_empty()
                                    }
                                    crate::characteristics::ProtectionFromQuality::Monocolored => {
                                        source.colors.len() == 1
                                    }
                                    crate::characteristics::ProtectionFromQuality::CardType(ct) => {
                                        source.card_types.contains(ct)
                                    }
                                    crate::characteristics::ProtectionFromQuality::Everything => {
                                        true
                                    }
                                    crate::characteristics::ProtectionFromQuality::Player(seat) => {
                                        source.controller == Some(*seat)
                                    }
                                },
                                _ => false,
                            })
                        })
            }
            TargetRef::Stack(_) => false,
        }
    }
    pub(super) fn spell_or_ability_source(
        &self,
        entry: &StackEntry,
        current: &EvaluatedCharacteristics,
    ) -> ProtectionSource {
        let mut out = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                let mut s = current
                    .get(source.id())
                    .map(ProtectionSource::from)
                    .or_else(|| {
                        self.departed_damage_sources
                            .get(&source.id())
                            .map(|source| ProtectionSource {
                                colors: source.colors.clone(),
                                creature_subtypes: source.creature_subtypes.clone(),
                                card_types: BTreeSet::new(),
                                is_creature: source.was_creature,
                                controller: None,
                                is_ability: true,
                                is_aura: false,
                            })
                    })
                    .or_else(|| {
                        self.execution_bindings(entry.id())
                            .and_then(|binding| binding.trigger.as_ref())
                            .map(|trigger| self.waiting_protection_source(&trigger.source, current))
                    })
                    .or_else(|| {
                        source
                            .base_program(&self.programs)
                            .map(ProtectionSource::from)
                    })
                    .unwrap_or_default();
                s.is_ability = true;
                s
            }
            StackKind::CardSpell(_) | StackKind::SpellCopy { .. } => self
                .window
                .spell_object(entry)
                .and_then(|id| current.get(id))
                .map(ProtectionSource::from)
                .or_else(|| match self.payloads.get(&entry.id()) {
                    Some(StackPayload::Instructions(payload)) => {
                        payload.copiable_traits.as_ref().map(ProtectionSource::from)
                    }
                    _ => None,
                })
                .unwrap_or_default(),
            _ => ProtectionSource {
                colors: self.targeting_entry_colors(entry, current),
                ..Default::default()
            },
        };
        out.controller = Some(entry.controller());
        out
    }

    pub(super) fn waiting_protection_source(
        &self,
        source: &SupportedAppearance,
        current: &EvaluatedCharacteristics,
    ) -> ProtectionSource {
        current
            .get(source.object.id())
            .map(ProtectionSource::from)
            .or_else(|| {
                self.departed_damage_sources
                    .get(&source.object.id())
                    .map(|source| ProtectionSource {
                        colors: source.colors.clone(),
                        creature_subtypes: source.creature_subtypes.clone(),
                        card_types: BTreeSet::new(),
                        is_creature: source.was_creature,
                        controller: None,
                        is_ability: false,
                        is_aura: false,
                    })
            })
            .unwrap_or_else(|| ProtectionSource {
                colors: source.colors.clone(),
                creature_subtypes: source.creature_subtypes.clone(),
                card_types: source.types.clone(),
                is_creature: source.types.contains(&CardType::Creature),
                controller: source.object.controller(),
                is_ability: false,
                is_aura: false,
            })
    }

    pub(super) fn targeting_entry_colors(
        &self,
        entry: &StackEntry,
        current: &EvaluatedCharacteristics,
    ) -> BTreeSet<Color> {
        match entry.kind() {
            StackKind::DelayedCopy { .. } => match self.payloads.get(&entry.id()) {
                Some(StackPayload::Instructions(p)) => match &p.delayed_copy_action {
                    Some(
                        crate::occurrence::DelayedCopyAction::SpellTwice(_, colors)
                        | crate::occurrence::DelayedCopyAction::AbilityTwice(_, colors),
                    ) => colors.clone(),
                    _ => BTreeSet::new(),
                },
                _ => BTreeSet::new(),
            },
            StackKind::Speed | StackKind::Monarch | StackKind::Emblem { .. } => BTreeSet::new(),
            StackKind::SpellCopy { .. } => self
                .window
                .spell_object(entry)
                .and_then(|id| current.get(id))
                .map(|view| view.current_colors.clone())
                .or_else(|| self.stack_copy_colors(entry.id()).cloned())
                .unwrap_or_default(),
            StackKind::CardSpell(id) => current
                .get(*id)
                .map(|s| s.current_colors.clone())
                .unwrap_or_default(),
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                current
                    .get(source.id())
                    .map(|s| s.current_colors.clone())
                    .or_else(|| {
                        self.departed_damage_sources
                            .get(&source.id())
                            .map(|s| s.colors.clone())
                    })
                    .or_else(|| {
                        self.execution_bindings(entry.id())?
                            .trigger
                            .as_ref()
                            .map(|t| t.source.colors.clone())
                    })
                    .unwrap_or_else(|| self.printed_source_colors(source))
            }
        }
    }
}
