//! CR 702.16e and 616.1. Each color prevents its matching source contributions to one recipient.
use super::*;
use crate::{characteristics::Color, damage::DamageAttemptId, object_identity::GameObject};

impl EffectWindow {
    pub(in crate::effects) fn printed_source_colors(&self, source: &GameObject) -> BTreeSet<Color> {
        source
            .base_program(&self.programs)
            .filter(|p| !p.printed_keywords.contains(&KeywordAbility::Devoid))
            .map(Program::defined_colors)
            .unwrap_or_default()
    }

    pub(in crate::effects) fn source_colors_for_damage(
        &self,
        evaluated: &EvaluatedCharacteristics,
        source: &DamageSource,
    ) -> BTreeSet<Color> {
        if let Some(reflection) = self.current_damage_reflection(source) {
            return reflection.colors.clone();
        }
        match source {
            DamageSource::Emblem { .. } => BTreeSet::new(),
            DamageSource::Object { object, .. } => evaluated
                .get(object.id())
                .map(|c| c.current_colors.clone())
                .or_else(|| {
                    self.departed_damage_sources
                        .get(&object.id())
                        .map(|s| s.colors.clone())
                })
                .unwrap_or_else(|| self.printed_source_colors(object)),
            DamageSource::SpellCopy { entry, .. } => {
                self.stack_copy_colors(*entry).cloned().unwrap_or_default()
            }
        }
    }

    pub(super) fn protection_applications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> BTreeMap<DamageApplication, Vec<DamageAttemptId>> {
        let mut applications = BTreeMap::<_, Vec<_>>::new();
        for component in batch
            .components()
            .filter(|component| component.can_prevent_damage())
        {
            let attempt = component.attempt();
            let colors = self.source_colors_for_damage(evaluated, &attempt.source);
            let id = match attempt.target {
                DamageRecipient::Creature(id)
                | DamageRecipient::Planeswalker(id)
                | DamageRecipient::Battle(id) => id,
                DamageRecipient::Player(player) => {
                    for color in evaluated
                        .player_protection_colors(player)
                        .filter(|color| colors.contains(color))
                    {
                        applications
                            .entry(DamageApplication {
                                effect: DamageEffectId::Protection(color),
                                scope: DamageScope::Recipient(attempt.target),
                            })
                            .or_default()
                            .push(attempt.id);
                    }
                    for active in &self.temporary_rules {
                        if active.consumed || !active.resolved_players.contains(&player) {
                            continue;
                        }
                        let crate::program::TemporaryRuleKind::PlayerGrantKeyword {
                            keywords, ..
                        } = &active.rule
                        else {
                            continue;
                        };
                        for kw in keywords {
                            let matched_effect = match kw {
                                KeywordAbility::ProtectionFromWhite
                                    if colors.contains(&Color::White) =>
                                {
                                    Some(DamageEffectId::Protection(Color::White))
                                }
                                KeywordAbility::ProtectionFromBlue
                                    if colors.contains(&Color::Blue) =>
                                {
                                    Some(DamageEffectId::Protection(Color::Blue))
                                }
                                KeywordAbility::ProtectionFromBlack
                                    if colors.contains(&Color::Black) =>
                                {
                                    Some(DamageEffectId::Protection(Color::Black))
                                }
                                KeywordAbility::ProtectionFromRed
                                    if colors.contains(&Color::Red) =>
                                {
                                    Some(DamageEffectId::Protection(Color::Red))
                                }
                                KeywordAbility::ProtectionFromGreen
                                    if colors.contains(&Color::Green) =>
                                {
                                    Some(DamageEffectId::Protection(Color::Green))
                                }
                                KeywordAbility::ProtectionFromMulticolored if colors.len() >= 2 => {
                                    Some(DamageEffectId::ProtectionMulticolored)
                                }
                                KeywordAbility::ProtectionFrom(
                                    crate::characteristics::ProtectionFromQuality::Everything,
                                ) => Some(DamageEffectId::ProtectionFrom(
                                    crate::characteristics::ProtectionFromQuality::Everything,
                                )),
                                KeywordAbility::ProtectionFrom(
                                    crate::characteristics::ProtectionFromQuality::Colorless,
                                ) if colors.is_empty() => Some(DamageEffectId::ProtectionFrom(
                                    crate::characteristics::ProtectionFromQuality::Colorless,
                                )),
                                KeywordAbility::ProtectionFrom(
                                    crate::characteristics::ProtectionFromQuality::Monocolored,
                                ) if colors.len() == 1 => Some(DamageEffectId::ProtectionFrom(
                                    crate::characteristics::ProtectionFromQuality::Monocolored,
                                )),
                                KeywordAbility::ProtectionFrom(
                                    crate::characteristics::ProtectionFromQuality::Player(seat),
                                ) if attempt.source.controller() == *seat => {
                                    Some(DamageEffectId::ProtectionFrom(
                                        crate::characteristics::ProtectionFromQuality::Player(
                                            *seat,
                                        ),
                                    ))
                                }
                                _ => None,
                            };
                            if let Some(effect) = matched_effect {
                                applications
                                    .entry(DamageApplication {
                                        effect,
                                        scope: DamageScope::Recipient(attempt.target),
                                    })
                                    .or_default()
                                    .push(attempt.id);
                            }
                        }
                    }
                    continue;
                }
            };
            let Some(target) = evaluated.get(id) else {
                continue;
            };
            let source_types =
                if let Some(reflection) = self.current_damage_reflection(&attempt.source) {
                    Some((&reflection.creature_types, reflection.is_creature))
                } else {
                    match &attempt.source {
                        DamageSource::Object { object, .. } => evaluated
                            .get(object.id())
                            .map(|source| (&source.current_creature_subtypes, source.is_creature()))
                            .or_else(|| {
                                self.departed_damage_sources
                                    .get(&object.id())
                                    .map(|source| (&source.creature_subtypes, source.was_creature))
                            }),
                        DamageSource::SpellCopy { entry, .. } => {
                            self.stack_copiable_creature_subtypes(*entry).zip(
                                self.stack_copiable_card_types(*entry)
                                    .map(|kinds| kinds.contains(&CardType::Creature)),
                            )
                        }
                        DamageSource::Emblem { .. } => None,
                    }
                };
            let (source_card_types, source_controller): (BTreeSet<CardType>, Option<SeatId>) =
                if let Some(reflection) = self.current_damage_reflection(&attempt.source) {
                    (
                        reflection.card_types.clone(),
                        match &reflection.source {
                            DamageSource::Emblem { controller, .. }
                            | DamageSource::Object { controller, .. }
                            | DamageSource::SpellCopy { controller, .. } => Some(*controller),
                        },
                    )
                } else {
                    match &attempt.source {
                        DamageSource::Object { object, controller } => {
                            let cts = evaluated
                                .get(object.id())
                                .map(|source| source.current_types.clone())
                                .or_else(|| {
                                    self.departed_damage_sources
                                        .get(&object.id())
                                        .map(|source| source.card_types.clone())
                                })
                                .or_else(|| {
                                    object
                                        .base_program(&self.programs)
                                        .map(|p| p.card_types.clone())
                                })
                                .unwrap_or_default();
                            (cts, Some(*controller))
                        }
                        DamageSource::SpellCopy { entry, controller } => (
                            self.stack_copiable_card_types(*entry)
                                .cloned()
                                .unwrap_or_default(),
                            Some(*controller),
                        ),
                        DamageSource::Emblem { controller, .. } => {
                            (BTreeSet::new(), Some(*controller))
                        }
                    }
                };
            for keyword in &target.keywords {
                if let KeywordAbility::ProtectionFromSubtype(quality) = keyword
                    && source_types
                        .is_some_and(|(types, creature)| quality.matches(types, creature))
                {
                    applications
                        .entry(DamageApplication {
                            effect: DamageEffectId::ProtectionSubtype(*quality),
                            scope: DamageScope::Recipient(attempt.target),
                        })
                        .or_default()
                        .push(attempt.id);
                } else if let KeywordAbility::ProtectionFrom(quality) = keyword {
                    let matched = match quality {
                        crate::characteristics::ProtectionFromQuality::Colorless => {
                            colors.is_empty()
                        }
                        crate::characteristics::ProtectionFromQuality::Monocolored => {
                            colors.len() == 1
                        }
                        crate::characteristics::ProtectionFromQuality::CardType(ct) => {
                            source_card_types.contains(ct)
                        }
                        crate::characteristics::ProtectionFromQuality::Everything => true,
                        crate::characteristics::ProtectionFromQuality::Player(seat) => {
                            source_controller == Some(*seat)
                        }
                    };
                    if matched {
                        applications
                            .entry(DamageApplication {
                                effect: DamageEffectId::ProtectionFrom(*quality),
                                scope: DamageScope::Recipient(attempt.target),
                            })
                            .or_default()
                            .push(attempt.id);
                    }
                }
            }
            if colors.len() >= 2 && target.has_keyword(KeywordAbility::ProtectionFromMulticolored) {
                applications
                    .entry(DamageApplication {
                        effect: DamageEffectId::ProtectionMulticolored,
                        scope: DamageScope::Recipient(attempt.target),
                    })
                    .or_default()
                    .push(attempt.id);
            }
            for color in colors {
                if target.protected_from_colors(&BTreeSet::from([color])) {
                    applications
                        .entry(DamageApplication {
                            effect: DamageEffectId::Protection(color),
                            scope: DamageScope::Recipient(attempt.target),
                        })
                        .or_default()
                        .push(attempt.id);
                }
            }
        }
        applications
    }

    pub(super) fn prevent_by_protection(
        &mut self,
        application: DamageApplication,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let eligible = self
            .protection_applications(&before.view().evaluated, batch)
            .remove(&application)
            .ok_or(DamageError::InapplicableEffect)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let prevention = work.batch.prevent_by_protection(application, eligible)?;
        self.emit_from(
            EventCause::DamageAdjusted {
                batch: prevention.batch,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::ProtectionPrevented(
                    prevention.clone(),
                )])
            },
        )?;
        journal.protections.push(prevention);
        Ok(())
    }
}
