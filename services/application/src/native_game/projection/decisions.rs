//! Actual frame-to-description mapping. Private native values never become wire IDs.
use super::*;
use spellwake_engine::{
    checks::CheckDecision,
    combat::CombatDecision,
    damage::{DamageEffectId, DamageRecipient},
    effects::{CopyTargetChoice, DamageDecision, EffectError, OptionalEffectChoice},
    game::{DecisionReply, GameCommand, GameError, GameInput, RuleFrame},
    opening::{MulliganChoice, OpeningDecision},
    targets::TargetRef,
};
impl Projection<'_, '_> {
    fn spell_offer_domain(
        &mut self,
        offer: &spellwake_engine::effects::ExileCastOffer,
    ) -> Result<PlayerDomain, NativeProjectionError> {
        let targets = offer
            .targets
            .iter()
            .flatten()
            .map(|t| self.target(*t))
            .collect::<Result<_, _>>()?;
        Ok(match offer.ability {
            spellwake_engine::effects::ExileCastAbility::Suspend => PlayerDomain::SuspendedCast {
                allows_x: offer.allows_x,
                spell: self.visible_object(offer.spell)?,
                target_required: offer.targets.is_some(),
                targets,
            },
            spellwake_engine::effects::ExileCastAbility::Miracle
            | spellwake_engine::effects::ExileCastAbility::RevealedLibrary
            | spellwake_engine::effects::ExileCastAbility::TargetedGraveyard
            | spellwake_engine::effects::ExileCastAbility::Commander
            | spellwake_engine::effects::ExileCastAbility::Siege
            | spellwake_engine::effects::ExileCastAbility::FoundSpell
            | spellwake_engine::effects::ExileCastAbility::MatchingHand
            | spellwake_engine::effects::ExileCastAbility::Impulse => {
                let price = offer
                    .normal_price
                    .as_ref()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let modes = offer
                    .modes
                    .as_ref()
                    .map(
                        |spec| -> Result<PlayerImpulseModes, NativeProjectionError> {
                            Ok(PlayerImpulseModes {
                                entwine_index: spec.entwine_index,
                                minimum: spec.minimum,
                                maximum: spec.maximum,
                                allow_repeat: spec.allow_repeat,
                                modes: spec
                                    .modes
                                    .iter()
                                    .map(|mode| {
                                        Ok(PlayerImpulseMode {
                                            multiple: mode.multiple.map(|(min, max)| [min, max]),
                                            index: mode.index,
                                            target_required: mode.targets.is_some(),
                                            targets: mode
                                                .targets
                                                .iter()
                                                .flatten()
                                                .map(|target| self.target(*target))
                                                .collect::<Result<_, _>>()?,
                                        })
                                    })
                                    .collect::<Result<_, NativeProjectionError>>()?,
                            })
                        },
                    )
                    .transpose()?;
                let permanent_targets = offer
                    .permanent_targets
                    .as_ref()
                    .map(
                        |spec| -> Result<PlayerImpulsePermanentTargets, NativeProjectionError> {
                            Ok(PlayerImpulsePermanentTargets {
                                division_total: spec.division_total,
                                minimum: u16::try_from(spec.minimum)
                                    .map_err(|_| NativeProjectionError::MissingNativeValue)?,
                                maximum: u16::try_from(spec.maximum)
                                    .map_err(|_| NativeProjectionError::MissingNativeValue)?,
                                targets: spec
                                    .targets
                                    .iter()
                                    .map(|target| self.target(*target))
                                    .collect::<Result<_, _>>()?,
                            })
                        },
                    )
                    .transpose()?;
                PlayerDomain::ImpulseCast {
                    spell: self.visible_object(offer.spell)?,
                    target_required: offer.targets.is_some(),
                    targets,
                    hybrid_symbols: offer
                        .hybrid_symbols
                        .iter()
                        .map(|symbol| crate::player_wire::PlayerHybridSymbol {
                            first: mana(symbol.first),
                            alternate_color: match symbol.alternative {
                                spellwake_engine::mana::HybridAlternative::Mana(color) => {
                                    Some(mana(color))
                                }
                                spellwake_engine::mana::HybridAlternative::TwoLife
                                | spellwake_engine::mana::HybridAlternative::TwoGeneric => None,
                            },
                        })
                        .collect(),
                    allows_x: offer.allows_x,
                    allows_alternative: offer.allows_alternative,
                    alternative_targetless: offer.alternative_targetless,
                    alternative_price: offer
                        .alternative_price
                        .as_ref()
                        .map(
                            |price| -> Result<PlayerImpulsePrice, NativeProjectionError> {
                                Ok(PlayerImpulsePrice {
                                    generic: quantity(&price.generic)?,
                                    required: price
                                        .required
                                        .iter()
                                        .map(|(&kind, amount)| {
                                            Ok(ManaSpend {
                                                color: mana(kind),
                                                amount: quantity(amount)?,
                                            })
                                        })
                                        .collect::<Result<_, NativeProjectionError>>()?,
                                })
                            },
                        )
                        .transpose()?,
                    modes,
                    permanent_targets,
                    fixed_targets: offer
                        .fixed_targets
                        .as_ref()
                        .map(|clauses| {
                            clauses
                                .iter()
                                .map(|targets| {
                                    targets
                                        .iter()
                                        .map(|target| self.target(*target))
                                        .collect::<Result<Vec<_>, _>>()
                                })
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .transpose()?,
                    generic: quantity(&price.generic)?,
                    required: price
                        .required
                        .iter()
                        .map(|(&kind, n)| {
                            Ok(ManaSpend {
                                color: mana(kind),
                                amount: quantity(n)?,
                            })
                        })
                        .collect::<Result<_, NativeProjectionError>>()?,
                }
            }
            spellwake_engine::effects::ExileCastAbility::Madness => {
                let price = offer
                    .madness_price
                    .as_ref()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                PlayerDomain::MadnessCast {
                    allows_x: offer.allows_x,
                    spell: self.visible_object(offer.spell)?,
                    target_required: offer.targets.is_some(),
                    targets,
                    generic: quantity(&price.generic)?,
                    required: price
                        .required
                        .iter()
                        .map(|(&kind, n)| {
                            Ok(ManaSpend {
                                color: mana(kind),
                                amount: quantity(n)?,
                            })
                        })
                        .collect::<Result<_, NativeProjectionError>>()?,
                }
            }
            spellwake_engine::effects::ExileCastAbility::Cascade => PlayerDomain::CascadeCast {
                allows_x: offer.allows_x,
                spell: self.visible_object(offer.spell)?,
                target_required: offer.targets.is_some(),
                targets,
            },
            spellwake_engine::effects::ExileCastAbility::Rebound => PlayerDomain::ReboundCast {
                allows_x: offer.allows_x,
                spell: self.visible_object(offer.spell)?,
                target_required: offer.targets.is_some(),
                targets,
            },
        })
    }
    fn entry_choice_domain(
        &mut self,
        rule: spellwake_engine::land::EntryTap,
        candidates: &[ObjectId],
        names: &[spellwake_contracts::names::EnglishCardName],
    ) -> Result<(DecisionKind, PlayerDomain), NativeProjectionError> {
        Ok(match rule {
            spellwake_engine::land::EntryTap::ReadAhead { final_chapter } => (
                DecisionKind::Number,
                PlayerDomain::Number {
                    minimum: 1.into(),
                    maximum: i64::from(final_chapter).into(),
                    mana_split: None,
                },
            ),
            spellwake_engine::land::EntryTap::SiegeProtector
            | spellwake_engine::land::EntryTap::ChosenOpponentControls
            | spellwake_engine::land::EntryTap::ChooseOpponent => {
                let eligible = self
                    .scope
                    .source
                    .execution()
                    .opponents_of(self.scope.binding.actor)
                    .map_err(|_| NativeProjectionError::MissingNativeValue)?;
                let targets = eligible
                    .into_iter()
                    .map(|seat| self.target(TargetRef::Player(seat)))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::OpponentChoice,
                    PlayerDomain::Targets { targets },
                )
            }
            spellwake_engine::land::EntryTap::ChoosePlayer => {
                let seats = self
                    .scope
                    .source
                    .execution()
                    .window()
                    .participants()
                    .to_vec();
                let targets = seats
                    .into_iter()
                    .map(|seat| self.target(TargetRef::Player(seat)))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::OpponentChoice,
                    PlayerDomain::Targets { targets },
                )
            }
            spellwake_engine::land::EntryTap::ChooseTwoPlayers => {
                let seats = self
                    .scope
                    .source
                    .execution()
                    .window()
                    .participants()
                    .to_vec();
                let targets = seats
                    .into_iter()
                    .map(|seat| self.target(TargetRef::Player(seat)))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::EntryPlayers,
                    PlayerDomain::TargetList {
                        secondary_targets: vec![],
                        targets,
                        minimum: count(2)?,
                        maximum: count(2)?,
                    },
                )
            }
            spellwake_engine::land::EntryTap::ChooseNonlandCardName => {
                let options = names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        self.option(
                            NativeReference::CardName(spellwake_engine::land::CardNameIndex(index)),
                            name.as_str().to_owned(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::CardName,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            spellwake_engine::land::EntryTap::ChooseCreatureType => {
                let vocabulary = self.scope.source.execution().creature_vocabulary();
                let options = vocabulary
                    .members()
                    .iter()
                    .map(|label| {
                        let id = vocabulary
                            .resolve_label(label)
                            .ok_or(NativeProjectionError::MissingNativeValue)?;
                        self.option(NativeReference::CreatureType(id), label.clone())
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::CreatureType,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            spellwake_engine::land::EntryTap::AlwaysChooseColorExcept(excluded)
            | spellwake_engine::land::EntryTap::ChooseColorExcept(excluded) => {
                let options = spellwake_engine::mana::ManaType::ALL
                    .into_iter()
                    .filter(|color| {
                        *color != excluded && *color != spellwake_engine::mana::ManaType::Colourless
                    })
                    .map(|color| {
                        self.option(
                            NativeReference::ManaColor(color),
                            mana_label(color).to_owned(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::ManaColor,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            spellwake_engine::land::EntryTap::ChooseColor => {
                let options = spellwake_engine::mana::ManaType::ALL
                    .into_iter()
                    .filter(|color| *color != spellwake_engine::mana::ManaType::Colourless)
                    .map(|color| {
                        self.option(
                            NativeReference::ManaColor(color),
                            mana_label(color).to_owned(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::ManaColor,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            other if other.option_labels().is_some() => {
                let labels = other.option_labels().unwrap_or(&[]);
                let options = labels
                    .iter()
                    .enumerate()
                    .map(|(idx, label)| {
                        let idx_u8 = u8::try_from(idx)
                            .map_err(|_| NativeProjectionError::MissingNativeValue)?;
                        self.option(NativeReference::OptionIndex(idx_u8), (*label).to_owned())
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::Modes,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            spellwake_engine::land::EntryTap::UnlessRevealHandLandType(_, _)
            | spellwake_engine::land::EntryTap::UnlessRevealHandSubtype { .. } => (
                DecisionKind::EntryReveal,
                self.objects_range(candidates, 0, candidates.len().min(1))?,
            ),
            spellwake_engine::land::EntryTap::UnlessPayLife(life) => {
                let options = vec![
                    self.option(
                        NativeReference::EntryPayLife(true),
                        format!("Pay {life} life (enter untapped)"),
                    )?,
                    self.option(
                        NativeReference::EntryPayLife(false),
                        "Decline (enter tapped)".to_owned(),
                    )?,
                ];
                (
                    DecisionKind::OptionalEffect,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            _ => return Err(NativeBridgeError::Unsupported.into()),
        })
    }
    fn option(
        &mut self,
        reference: NativeReference,
        label: String,
    ) -> Result<PlayerOption, NativeProjectionError> {
        Ok(PlayerOption {
            handle: self.issue(reference)?,
            label,
        })
    }
    fn objects_domain(
        &mut self,
        ids: &[ObjectId],
        required: usize,
    ) -> Result<PlayerDomain, NativeProjectionError> {
        self.objects_range(ids, required, required)
    }
    fn objects_range(
        &mut self,
        ids: &[ObjectId],
        minimum: usize,
        maximum: usize,
    ) -> Result<PlayerDomain, NativeProjectionError> {
        let objects = ids
            .iter()
            .map(|id| self.visible_object(*id))
            .collect::<Result<_, _>>()?;
        Ok(PlayerDomain::Objects {
            prior_choices: vec![],
            prior_votes: vec![],
            objects,
            minimum: count(minimum)?,
            maximum: count(maximum)?,
        })
    }
    fn object_order(&mut self, ids: &[ObjectId]) -> Result<PlayerDomain, NativeProjectionError> {
        let objects = ids
            .iter()
            .map(|id| self.visible_object(*id))
            .collect::<Result<Vec<_>, _>>()?;
        let labels = objects
            .iter()
            .map(|handle| {
                Ok(PlayerOption {
                    handle: handle.clone(),
                    label: self.object_label(handle)?,
                })
            })
            .collect::<Result<_, NativeProjectionError>>()?;
        Ok(PlayerDomain::Ordering { objects, labels })
    }
    fn recipient(&mut self, value: DamageRecipient) -> Result<PlayerTarget, NativeProjectionError> {
        self.target(match value {
            DamageRecipient::Creature(id)
            | DamageRecipient::Planeswalker(id)
            | DamageRecipient::Battle(id) => TargetRef::Object(id),
            DamageRecipient::Player(seat) => TargetRef::Player(seat),
        })
    }
    fn target_label(&mut self, target: TargetRef) -> Result<String, NativeProjectionError> {
        let state = self.scope.source.execution();
        if let TargetRef::Stack(id) = target
            && let Some(entry) = state.window().resolving().filter(|entry| entry.id() == id)
        {
            return self
                .display
                .stack(entry, state.stack_body(id), state.execution_bindings(id));
        }
        let value = self.target(target)?;
        match value {
            PlayerTarget::Object { handle } => self.object_label(&handle),
            PlayerTarget::Stack { handle } => self
                .stack
                .iter()
                .find(|entry| entry.handle == handle)
                .map(|entry| entry.label.clone())
                .ok_or(NativeProjectionError::MissingNativeValue),
            PlayerTarget::Seat { seat_id } => self
                .scope
                .binding
                .seats
                .iter()
                .position(|entry| entry.seat_id == seat_id)
                .map(|index| format!("Player {}", index + 1))
                .ok_or(NativeProjectionError::MissingNativeValue),
        }
    }
    fn revealed_library_card_name(&self, id: ObjectId) -> Result<String, NativeProjectionError> {
        let state = self.scope.source.execution();
        let object = state
            .window()
            .cards()
            .get(id)
            .ok_or(NativeProjectionError::MissingNativeValue)?;
        let program = state
            .copiable_program(id)
            .ok_or(NativeProjectionError::MissingNativeValue)?;
        Ok(self.display.object(object, program)?.characteristics.name)
    }
    fn copy_probe(&self, choice: CopyTargetChoice) -> Result<bool, NativeProjectionError> {
        let source = self.scope.source;
        let pending = source
            .pending_decision()
            .ok_or(NativeProjectionError::MissingNativeValue)?;
        match source.advance(GameInput {
            basis: source.basis().clone(),
            actor: self.scope.binding.actor,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::CopyTargets(choice),
            },
        }) {
            Ok(_) => Ok(true),
            Err(GameError::Effect(EffectError::IllegalTarget)) => Ok(false),
            Err(cause) => Err(NativeBridgeError::Native(Box::new(cause)).into()),
        }
    }
    pub(super) fn decisions(&mut self) -> Result<Vec<PlayerDecision>, NativeProjectionError> {
        let source = self.scope.source;
        crate::native_game::clocks::supported_wait(source)?;
        let Some(pending) = source.pending_decision() else {
            return Ok(vec![]);
        };
        if pending.chooser() != self.scope.binding.actor {
            return Ok(vec![]);
        }
        let (kind, domain) = match pending.frame() {
            RuleFrame::ZoneReplacement(frame) => {
                use spellwake_engine::effects::ZoneReplacementRequest;
                let mut options = Vec::new();
                let applications = match frame.request() {
                    ZoneReplacementRequest::Devour { choices, .. }
                    | ZoneReplacementRequest::DevourGraveyardOrder { choices } => {
                        let selecting =
                            matches!(frame.request(), ZoneReplacementRequest::Devour { .. });
                        if selecting {
                            options.push(self.option(
                                NativeReference::ZoneReplacement(0),
                                "Finish choosing creatures to sacrifice".to_owned(),
                            )?);
                        }
                        for (index, &id) in choices.iter().enumerate() {
                            let label = self.target_label(TargetRef::Object(id))?;
                            let label = if selecting {
                                format!("Sacrifice {label}")
                            } else {
                                format!("Place {label} next in graveyard order")
                            };
                            options.push(self.option(
                                NativeReference::ZoneReplacement(index + usize::from(selecting)),
                                label,
                            )?);
                        }
                        &[]
                    }
                    ZoneReplacementRequest::EntryCopy {
                        donors,
                        battlefield_donors,
                        ..
                    } => {
                        if !donors.is_empty() {
                            options.push(self.option(
                                NativeReference::ZoneReplacement(0),
                                "Decline to copy".to_owned(),
                            )?);
                        }
                        for (index, &donor) in donors.iter().enumerate() {
                            let label = self.target_label(TargetRef::Object(donor))?;
                            options.push(self.option(
                                NativeReference::ZoneReplacement(index + 1),
                                format!("Copy {label}"),
                            )?);
                        }
                        let offset = options.len();
                        for (index, &(aura, host)) in battlefield_donors.iter().enumerate() {
                            let aura = self.target_label(TargetRef::Object(aura))?;
                            let host = self.target_label(TargetRef::Object(host))?;
                            options.push(self.option(
                                NativeReference::ZoneReplacement(index + offset),
                                format!("Copy {host} with {aura}"),
                            )?);
                        }
                        &[]
                    }
                    ZoneReplacementRequest::EntryController { opponents, .. } => {
                        for (index, seat) in opponents.iter().copied().enumerate() {
                            let label = self.target_label(TargetRef::Player(seat))?;
                            options.push(self.option(
                                NativeReference::ZoneReplacement(index),
                                format!("Enter under {label}’s control"),
                            )?);
                        }
                        &[]
                    }
                    ZoneReplacementRequest::Select { applications } => applications.as_slice(),
                    ZoneReplacementRequest::ConfirmOptional { application } => {
                        std::slice::from_ref(application)
                    }
                };
                // A hidden subject or source must never disclose an identity through a label.
                for (index, application) in applications.iter().enumerate() {
                    let subject = match self.visible_object(application.event.before()) {
                        Ok(object) => self.object_label(&object)?,
                        Err(NativeProjectionError::HiddenReference) => {
                            let id = application.event.before();
                            let authorized = self
                                .scope
                                .source
                                .execution()
                                .pending_search_entry_subjects()
                                .is_some_and(|(chooser, subjects)| {
                                    chooser == self.scope.binding.actor && subjects.contains(&id)
                                });
                            if authorized {
                                let object = self
                                    .scope
                                    .source
                                    .execution()
                                    .window()
                                    .cards()
                                    .get(id)
                                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                                let program = self
                                    .scope
                                    .source
                                    .execution()
                                    .base_program(id)
                                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                                self.display.object(object, program)?.characteristics.name
                            } else {
                                "the affected card".to_owned()
                            }
                        }
                        Err(error) => return Err(error),
                    };
                    let effect = if application.effect.created
                        == Some(spellwake_engine::effects::CreatedZoneEffect::CorpseReturn)
                    {
                        "exile the returned creature".to_owned()
                    } else if matches!(
                        application.effect.created,
                        Some(spellwake_engine::effects::CreatedZoneEffect::TemporaryLeaveExile(_))
                    ) {
                        "exile if it would leave the battlefield".to_owned()
                    } else if application.effect.madness {
                        "madness".to_owned()
                    } else if application.effect.unearth {
                        "unearth".to_owned()
                    } else {
                        match self.visible_object(application.effect.source) {
                            Ok(source) => {
                                let label = self.object_label(&source)?;
                                let description = self
                                    .scope
                                    .source
                                    .execution()
                                    .copiable_program(application.effect.source)
                                    .and_then(|program| {
                                        program.zone_replacements.iter().find(|ability| {
                                            ability.id == application.effect.ability
                                        })
                                    })
                                    .map(|ability| {
                                        format!(
                                            " ({:?} to {:?})",
                                            ability.destination, ability.replace_with
                                        )
                                    })
                                    .unwrap_or_default();
                                format!("{label}{description}")
                            }
                            Err(NativeProjectionError::HiddenReference) => {
                                "a replacement effect".to_owned()
                            }
                            Err(error) => return Err(error),
                        }
                    };
                    options.push(self.option(
                        NativeReference::ZoneReplacement(index),
                        format!("Apply {effect} to {subject}"),
                    )?);
                    if matches!(
                        frame.request(),
                        ZoneReplacementRequest::ConfirmOptional { .. }
                    ) {
                        options.push(self.option(
                            NativeReference::ZoneReplacement(1),
                            format!("Decline {effect} for {subject}"),
                        )?);
                    }
                }
                (
                    DecisionKind::ZoneReplacement,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::Checks(CheckDecision::CleanupSelect(frame)) => (
                DecisionKind::Discard,
                self.objects_domain(frame.domain(), frame.required())?,
            ),
            RuleFrame::Mutation(_) => (
                DecisionKind::Number,
                PlayerDomain::Number {
                    minimum: 1.into(),
                    maximum: 2.into(),
                    mana_split: None,
                },
            ),
            RuleFrame::LibraryTop(frame) if frame.chooses_land_kind() => (
                DecisionKind::Number,
                PlayerDomain::Number {
                    minimum: 1.into(),
                    maximum: 2.into(),
                    mana_split: None,
                },
            ),
            RuleFrame::LibraryTop(frame)
                if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Search =>
            {
                let mut inspected = Vec::new();
                let mut eligible = Vec::new();
                let mut entry_color_offers = Vec::new();
                let mut entry_reveal_offers = Vec::new();
                let matching: std::collections::BTreeSet<_> =
                    frame.eligible_results().iter().copied().collect();
                let library = frame.domain_top_first();
                let library_objects: std::collections::BTreeSet<_> =
                    library.iter().copied().collect();
                for id in library.iter().chain(matching.difference(&library_objects)) {
                    let physical = source
                        .execution()
                        .window()
                        .cards()
                        .get(*id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let program = source
                        .execution()
                        .copiable_program(*id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let label = self.display.object(physical, program)?.characteristics.name;
                    let label = if library_objects.contains(id) {
                        label
                    } else {
                        format!("{label} (graveyard)")
                    };
                    if matching.contains(id) {
                        let option = self.option(NativeReference::Object(*id), label.clone())?;
                        if let Some(colors) = frame.search_entry_color_options().get(id) {
                            entry_color_offers.push(PlayerSearchColorOffer {
                                object: option.handle.clone(),
                                colors: colors.iter().copied().map(mana).collect(),
                            });
                        }
                        if let Some(cards) = frame.search_entry_reveal_options().get(id) {
                            let cards = cards
                                .iter()
                                .map(|card| self.visible_object(*card))
                                .collect::<Result<Vec<_>, _>>()?;
                            entry_reveal_offers.push(PlayerSearchRevealOffer {
                                object: option.handle.clone(),
                                cards,
                            });
                        }
                        eligible.push(option);
                    }
                    inspected.push(label);
                }
                (
                    DecisionKind::LibrarySearch,
                    PlayerDomain::LibrarySearch {
                        shared_land_type: frame.requires_shared_land_type(),
                        forest_and_plains: frame.requires_forest_and_plains(),
                        maximum: frame
                            .is_bounded_batch_search()
                            .then(|| frame.search_batch_limit()),
                        mode: if frame.is_look_top() && frame.is_battlefield_batch_search() {
                            PlayerSearchMode::BattlefieldTimestampOrder
                        } else if frame.may_put_explored_card_in_graveyard() {
                            PlayerSearchMode::Explore
                        } else if frame.is_look_top() && !frame.is_bounded_batch_search() {
                            PlayerSearchMode::LookTop
                        } else if frame.is_hand_graveyard_search() {
                            PlayerSearchMode::HandGraveyard
                        } else if frame.is_exile_batch_search() {
                            PlayerSearchMode::ExileBatch
                        } else if frame.is_bounded_batch_search() {
                            PlayerSearchMode::HandBatch
                        } else if frame.offers_search_destination() {
                            PlayerSearchMode::BattlefieldOrHand
                        } else if frame.is_split_search() {
                            PlayerSearchMode::BattlefieldThenHand
                        } else if frame.is_battlefield_batch_search() {
                            PlayerSearchMode::BattlefieldTimestampOrder
                        } else {
                            PlayerSearchMode::Single
                        },
                        required: frame.requires_search_result(),
                        entry_color_offers,
                        entry_reveal_offers,
                        inspected,
                        eligible,
                    },
                )
            }
            RuleFrame::LibraryTop(frame)
                if matches!(
                    frame.kind(),
                    spellwake_engine::effects::LibraryChoiceKind::Scry
                        | spellwake_engine::effects::LibraryChoiceKind::Surveil
                ) =>
            {
                let mut inspected = Vec::with_capacity(frame.domain_top_first().len());
                for id in frame.domain_top_first() {
                    let physical = source
                        .execution()
                        .window()
                        .cards()
                        .get(*id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let program = source
                        .execution()
                        .copiable_program(*id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let label = self.display.object(physical, program)?.characteristics.name;
                    inspected.push(self.option(NativeReference::Object(*id), label)?);
                }
                let kind = if frame.kind() == spellwake_engine::effects::LibraryChoiceKind::Scry {
                    DecisionKind::Scry
                } else {
                    DecisionKind::Surveil
                };
                (kind, PlayerDomain::LibraryTop { inspected })
            }
            RuleFrame::LibraryTop(_) => return Err(NativeBridgeError::Unsupported.into()),
            RuleFrame::ResolutionSelect(frame) => {
                let mut domain =
                    self.objects_range(frame.domain(), frame.minimum(), frame.required())?;
                if let PlayerDomain::Objects { prior_choices, .. } = &mut domain {
                    for (seat, object) in source.execution().prior_public_exile_choices() {
                        prior_choices.push(PlayerCardVote {
                            seat_id: self.app_seat(seat)?,
                            object: self.visible_object(object)?,
                        });
                    }
                }
                (DecisionKind::SelectResolution, domain)
            }
            RuleFrame::CounterReplacement(frame) => (
                DecisionKind::Number,
                PlayerDomain::Number {
                    minimum: 0.into(),
                    maximum: RulesInteger::from_usize(frame.doublers()),
                    mana_split: None,
                },
            ),
            RuleFrame::TransformOrder(f) => (
                DecisionKind::TransformOrder,
                self.object_order(f.objects())?,
            ),
            RuleFrame::UntapKeepTapped(f) => (
                DecisionKind::SelectResolution,
                self.objects_range(f.objects(), 0, f.objects().len())?,
            ),
            RuleFrame::LinkedReturnOrder(f) if source.execution().is_looked_graveyard_order(f) => {
                let mut objects = Vec::new();
                let mut labels = Vec::new();
                for id in f.objects() {
                    let card = source
                        .execution()
                        .window()
                        .cards()
                        .get(*id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let program = source
                        .execution()
                        .copiable_program(*id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let label = self.display.object(card, program)?.characteristics.name;
                    let option = self.option(NativeReference::Object(*id), label)?;
                    objects.push(option.handle.clone());
                    labels.push(option);
                }
                (
                    DecisionKind::LinkedReturnOrder,
                    PlayerDomain::Ordering { objects, labels },
                )
            }
            RuleFrame::LinkedReturnOrder(f) => (
                DecisionKind::LinkedReturnOrder,
                self.object_order(f.objects())?,
            ),
            RuleFrame::ResolutionGraveyardOrder(frame) => (
                DecisionKind::OrderResolutionGraveyard,
                self.object_order(frame.domain())?,
            ),
            RuleFrame::OptionalEffect(frame) if frame.exile_cast().is_some() => {
                let offer = frame
                    .exile_cast()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let domain = if let Some(right) = &offer.split_right {
                    PlayerDomain::SplitCast {
                        left: if offer.normal_available {
                            Some(Box::new(self.spell_offer_domain(offer)?))
                        } else {
                            None
                        },
                        right: Box::new(self.spell_offer_domain(right)?),
                    }
                } else if let Some(alternate) = &offer.adventure {
                    PlayerDomain::AdventureCast {
                        normal: if offer.normal_available {
                            Some(Box::new(self.spell_offer_domain(offer)?))
                        } else {
                            None
                        },
                        adventure: Box::new(self.spell_offer_domain(alternate)?),
                    }
                } else {
                    self.spell_offer_domain(offer)?
                };
                (DecisionKind::OptionalEffect, domain)
            }
            RuleFrame::OptionalEffect(frame) => {
                let options =
                    frame
                        .options()
                        .iter()
                        .map(|choice| {
                            if let OptionalEffectChoice::PermanentTypeOrder(order) = choice {
                                let labels = order.map(|kind| match kind {
                                    spellwake_engine::program::CardType::Artifact => "artifacts",
                                    spellwake_engine::program::CardType::Creature => "creatures",
                                    spellwake_engine::program::CardType::Land => "lands",
                                    _ => "permanents",
                                });
                                return self.option(
                                    NativeReference::OptionalEffect(*choice),
                                    labels.join(", then "),
                                );
                            }
                            self.option(
                                NativeReference::OptionalEffect(*choice),
                                match choice {
                                    OptionalEffectChoice::Take if frame.rebound_or_buyback() => {
                                        if source.execution().window().resolving().is_some_and(
                                            |entry| entry.exiles_if_put_into_graveyard(),
                                        ) {
                                            "Exile the spell"
                                        } else {
                                            "Exile with rebound"
                                        }
                                    }
                                    OptionalEffectChoice::Decline if frame.rebound_or_buyback() => {
                                        "Return to hand with buyback"
                                    }
                                    OptionalEffectChoice::PermanentType(
                                        spellwake_engine::program::CardType::Artifact,
                                    ) => "Artifact",
                                    OptionalEffectChoice::PermanentType(
                                        spellwake_engine::program::CardType::Battle,
                                    ) => "Battle",
                                    OptionalEffectChoice::PermanentType(
                                        spellwake_engine::program::CardType::Creature,
                                    ) => "Creature",
                                    OptionalEffectChoice::PermanentType(
                                        spellwake_engine::program::CardType::Enchantment,
                                    ) => "Enchantment",
                                    OptionalEffectChoice::PermanentType(
                                        spellwake_engine::program::CardType::Land,
                                    ) => "Land",
                                    OptionalEffectChoice::PermanentType(
                                        spellwake_engine::program::CardType::Planeswalker,
                                    ) => "Planeswalker",
                                    OptionalEffectChoice::PermanentType(_) => {
                                        return Err(NativeProjectionError::MissingNativeValue);
                                    }
                                    OptionalEffectChoice::Take => "Take effect",
                                    OptionalEffectChoice::Decline => "Decline effect",
                                    OptionalEffectChoice::PermanentTypeOrder(_) => {
                                        return Err(NativeProjectionError::MissingNativeValue);
                                    }
                                }
                                .into(),
                            )
                        })
                        .collect::<Result<_, _>>()?;
                (
                    DecisionKind::OptionalEffect,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::ResolutionPayment(frame) => {
                let cost = frame.cost();
                let available = |kind| {
                    self.scope
                        .source
                        .execution()
                        .unrestricted_mana(frame.payer(), kind)
                };
                let total = if frame.variable() {
                    ManaType::ALL
                        .into_iter()
                        .fold(0.into(), |sum: spellwake_contracts::RulesInteger, kind| {
                            sum.plus(&available(kind))
                        })
                } else {
                    cost.total()
                };
                let minimum_spend = cost
                    .required
                    .iter()
                    .map(|(kind, amount)| {
                        Ok(ManaSpend {
                            color: mana(*kind),
                            amount: quantity(amount)?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                // Allocation bounds quote the exact cost, not current affordability.
                // A fresh pool is projected separately; native Pay owns that check.
                let maximum_spend = ManaType::ALL
                    .into_iter()
                    .filter(|color| {
                        frame
                            .restricted_colors()
                            .is_none_or(|allowed| allowed.contains(color))
                    })
                    .map(|kind| {
                        Ok(ManaSpend {
                            color: mana(kind),
                            amount: quantity(&if frame.variable() {
                                available(kind)
                            } else {
                                cost.total()
                            })?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::ResolutionPayment,
                    PlayerDomain::Payment {
                        minimum_spend,
                        maximum_spend,
                        total: quantity(&total)?,
                        allow_decline: true,
                        up_to: frame.variable(),
                    },
                )
            }
            RuleFrame::TimestampOrder(frame) => {
                let mut objects = vec![];
                let mut labels = vec![];
                for preview in frame.members() {
                    let label = self.display.token_preview(preview)?;
                    let handle = self.issue(NativeReference::TokenMember(preview.member))?;
                    objects.push(handle.clone());
                    labels.push(PlayerOption { handle, label });
                }
                (
                    DecisionKind::TimestampOrder,
                    PlayerDomain::Ordering { objects, labels },
                )
            }
            RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) => (
                DecisionKind::Discard,
                self.objects_domain(frame.domain(), frame.required())?,
            ),
            RuleFrame::Checks(CheckDecision::BattleProtector(frame)) => {
                let targets = frame
                    .domain()
                    .iter()
                    .map(|&seat| self.target(TargetRef::Player(seat)))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::OpponentChoice,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::Checks(CheckDecision::ReturnCommanders(frame)) => (
                DecisionKind::ReturnCommanders,
                self.objects_range(frame.domain(), 0, frame.domain().len())?,
            ),
            RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) => (
                DecisionKind::OrderGraveyard,
                self.object_order(frame.domain())?,
            ),
            RuleFrame::Checks(CheckDecision::Legend(frame)) => {
                let conflicts = frame
                    .conflicts()
                    .iter()
                    .map(|conflict| {
                        Ok(LegendDomain {
                            conflict: self.issue(NativeReference::Legend(conflict.id()))?,
                            objects: conflict
                                .domain()
                                .iter()
                                .map(|id| self.visible_object(*id))
                                .collect::<Result<_, _>>()?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (DecisionKind::KeepLegend, PlayerDomain::Legend { conflicts })
            }
            RuleFrame::Triggers(frame) => {
                let mut objects = vec![];
                let mut labels = vec![];
                for id in frame.instances() {
                    if source.execution().waiting_delayed_copy_trigger(*id) {
                        let handle = self.issue(NativeReference::Trigger(*id))?;
                        objects.push(handle.clone());
                        labels.push(PlayerOption {
                            handle,
                            label: "Delayed sacrifice".into(),
                        });
                        continue;
                    }
                    if source.execution().waiting_emblem_trigger(*id).is_some() {
                        let handle = self.issue(NativeReference::Trigger(*id))?;
                        objects.push(handle.clone());
                        labels.push(PlayerOption {
                            handle,
                            label: "Emblem ability".into(),
                        });
                        continue;
                    }
                    if let Some((_, beneficiary)) = source.execution().waiting_monarch_trigger(*id)
                    {
                        let handle = self.issue(NativeReference::Trigger(*id))?;
                        objects.push(handle.clone());
                        labels.push(PlayerOption {
                            handle,
                            label: if beneficiary.is_some() {
                                "Combat damage: become monarch"
                            } else {
                                "Monarch: draw a card"
                            }
                            .into(),
                        });
                        continue;
                    }
                    if source.execution().waiting_speed_trigger(*id).is_some() {
                        let handle = self.issue(NativeReference::Trigger(*id))?;
                        objects.push(handle.clone());
                        labels.push(PlayerOption {
                            handle,
                            label: "Increase speed".into(),
                        });
                        continue;
                    }
                    let instance = source
                        .execution()
                        .waiting_triggers()
                        .find(|value| value.id == *id)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    let label = if instance.source.object.face_down().is_some() {
                        "Triggered ability".into()
                    } else {
                        self.display.trigger(instance)?
                    };
                    let handle = self.issue(NativeReference::Trigger(*id))?;
                    objects.push(handle.clone());
                    labels.push(PlayerOption { handle, label });
                }
                (
                    DecisionKind::OrderTriggers,
                    PlayerDomain::Ordering { objects, labels },
                )
            }
            RuleFrame::RevealedPiles(frame) => {
                use spellwake_engine::effects::PileChoice;
                match frame.choice() {
                    PileChoice::Take { .. } => (
                        DecisionKind::Number,
                        PlayerDomain::Number {
                            minimum: 1.into(),
                            maximum: 2.into(),
                            mana_split: None,
                        },
                    ),
                    PileChoice::Divide(ids)
                    | PileChoice::BottomOrder(ids)
                    | PileChoice::GraveyardOrder(ids) => {
                        let mut options = Vec::with_capacity(ids.len());
                        for &id in ids {
                            let label = self.revealed_library_card_name(id)?;
                            options.push(self.option(NativeReference::Object(id), label)?);
                        }
                        if matches!(
                            frame.choice(),
                            PileChoice::BottomOrder(_) | PileChoice::GraveyardOrder(_)
                        ) {
                            (
                                DecisionKind::LinkedReturnOrder,
                                PlayerDomain::Ordering {
                                    objects: options
                                        .iter()
                                        .map(|option| option.handle.clone())
                                        .collect(),
                                    labels: options,
                                },
                            )
                        } else {
                            (
                                DecisionKind::LibrarySearch,
                                PlayerDomain::LibrarySearch {
                                    maximum: None,
                                    mode: PlayerSearchMode::DividePile,
                                    required: false,
                                    shared_land_type: false,
                                    forest_and_plains: false,
                                    inspected: options
                                        .iter()
                                        .map(|option| option.label.clone())
                                        .collect(),
                                    eligible: options,
                                    entry_color_offers: vec![],
                                    entry_reveal_offers: vec![],
                                },
                            )
                        }
                    }
                }
            }
            RuleFrame::DrawReplacement(frame) => {
                use spellwake_engine::effects::DrawReplacementDomain;
                match &frame.domain {
                    DrawReplacementDomain::Miracle { card } => {
                        let name = self.revealed_library_card_name(*card)?;
                        let options = vec![
                            self.option(
                                NativeReference::MiracleReveal(true),
                                format!("Reveal {name} for Miracle"),
                            )?,
                            self.option(
                                NativeReference::MiracleReveal(false),
                                "Draw without revealing".to_owned(),
                            )?,
                        ];
                        (
                            DecisionKind::OptionalEffect,
                            PlayerDomain::Options {
                                options,
                                query: None,
                            },
                        )
                    }
                    DrawReplacementDomain::LandKind => {
                        let options = [
                            (None, "Draw a card"),
                            (Some(true), "Land"),
                            (Some(false), "Nonland"),
                        ]
                        .into_iter()
                        .map(|(kind, label)| {
                            self.option(NativeReference::DrawLandKind(kind), label.into())
                        })
                        .collect::<Result<_, _>>()?;
                        (
                            DecisionKind::OptionalEffect,
                            PlayerDomain::Options {
                                options,
                                query: None,
                            },
                        )
                    }
                    DrawReplacementDomain::Collector(seats) => {
                        let targets = seats
                            .iter()
                            .map(|&seat| self.target(TargetRef::Player(seat)))
                            .collect::<Result<_, _>>()?;
                        (
                            DecisionKind::OpponentChoice,
                            PlayerDomain::Targets { targets },
                        )
                    }
                    DrawReplacementDomain::Bottom(objects) => {
                        let labels = objects
                            .iter()
                            .map(|&id| {
                                self.option(
                                    NativeReference::Object(id),
                                    self.revealed_library_card_name(id)?,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        (
                            DecisionKind::LinkedReturnOrder,
                            PlayerDomain::Ordering {
                                objects: labels
                                    .iter()
                                    .map(|option| option.handle.clone())
                                    .collect(),
                                labels,
                            },
                        )
                    }
                }
            }
            RuleFrame::DamageSourceChoice(frame) => {
                let mut options = Vec::with_capacity(frame.sources().len());
                for &damage_source in frame.sources() {
                    let label = match damage_source {
                        spellwake_engine::damage::DamageSourceId::Object(id) => {
                            if let Some(object) = source.execution().window().cards().get(id) {
                                if matches!(object.zone(), ObjectZone::Library(_))
                                    || matches!(object.zone(), ObjectZone::Hand(seat) if seat != self.scope.binding.actor)
                                {
                                    return Err(NativeProjectionError::HiddenReference);
                                }
                                if object.face_down().is_some() {
                                    "a face-down source".to_owned()
                                } else {
                                    let program =
                                        source.execution().copiable_program(id).or_else(|| {
                                            object.card_id().and_then(|cid| {
                                                source
                                                    .execution()
                                                    .programs()
                                                    .find(|(key, _)| *key == cid)
                                                    .map(|(_, p)| p)
                                            })
                                        });
                                    let base = if let Some(program) = program {
                                        self.display.object(object, program)?.characteristics.name
                                    } else {
                                        "a source".to_owned()
                                    };
                                    match object.zone() {
                                        ObjectZone::Stack => format!("{base} (spell)"),
                                        ObjectZone::Command => format!("{base} (command zone)"),
                                        _ => base,
                                    }
                                }
                            } else if let Some(departed) =
                                source.execution().departed_damage_sources().get(&id)
                            {
                                if departed.object.face_down().is_some() {
                                    "a face-down source".to_owned()
                                } else {
                                    let program =
                                        source.execution().copiable_program(id).or_else(|| {
                                            departed.object.card_id().and_then(|cid| {
                                                source
                                                    .execution()
                                                    .programs()
                                                    .find(|(key, _)| *key == cid)
                                                    .map(|(_, p)| p)
                                            })
                                        });
                                    if let Some(program) = program {
                                        format!(
                                            "{} (departed source)",
                                            self.display
                                                .object(&departed.object, program)?
                                                .characteristics
                                                .name
                                        )
                                    } else {
                                        "a departed source".to_owned()
                                    }
                                }
                            } else {
                                return Err(NativeProjectionError::MissingNativeValue);
                            }
                        }
                        spellwake_engine::damage::DamageSourceId::SpellCopy(entry_id) => {
                            if let Some(name) = source
                                .execution()
                                .stack_copiable_names(entry_id)
                                .and_then(|names| names.iter().next())
                            {
                                format!("{} (spell copy)", name.as_str())
                            } else {
                                "a spell copy".to_owned()
                            }
                        }
                        spellwake_engine::damage::DamageSourceId::Emblem(_) => {
                            "an emblem".to_owned()
                        }
                    };
                    options.push(self.option(NativeReference::DamageSource(damage_source), label)?);
                }
                (
                    DecisionKind::DamageSource,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::OpponentChoice(frame) => {
                let targets = frame
                    .opponents()
                    .iter()
                    .map(|seat| self.target(TargetRef::Player(*seat)))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::OpponentChoice,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::ReplacementChooser(frame) => {
                let targets = frame
                    .candidates()
                    .iter()
                    .map(|&seat| self.target(TargetRef::Player(seat)))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::ReplacementChooser,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::MyriadDefender(frame) => {
                let targets = frame
                    .defenders()
                    .map(|target| self.target(target))
                    .collect::<Result<Vec<_>, _>>()?;
                (
                    DecisionKind::MyriadDefender,
                    PlayerDomain::TargetList {
                        secondary_targets: vec![],
                        minimum: count(usize::from(!frame.may_decline()))?,
                        maximum: count(1)?,
                        targets,
                    },
                )
            }
            RuleFrame::ReselectAttackDefender(frame) => {
                let targets: Vec<_> = frame
                    .defenders()
                    .map(|target| self.target(target))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::ReselectAttackDefender,
                    PlayerDomain::TargetList {
                        secondary_targets: vec![],
                        minimum: count(0)?,
                        maximum: count(targets.len().min(1))?,
                        targets,
                    },
                )
            }
            RuleFrame::TokenDefender(frame) => {
                let targets = frame
                    .defenders()
                    .map(|target| self.target(target))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::TokenDefender,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::TriggerTarget(frame) if frame.modal().is_some() => {
                let modal = frame
                    .modal()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let modes = modal
                    .modes
                    .iter()
                    .map(|mode| {
                        Ok(crate::player_wire::PlayerImpulseMode {
                            index: mode.index,
                            target_required: mode.target_required,
                            multiple: mode.multiple.map(|(minimum, maximum)| [minimum, maximum]),
                            targets: mode
                                .targets
                                .iter()
                                .map(|target| self.target(*target))
                                .collect::<Result<_, _>>()?,
                        })
                    })
                    .collect::<Result<Vec<_>, NativeProjectionError>>()?;
                (
                    DecisionKind::Modes,
                    PlayerDomain::TriggerModes {
                        minimum: modal.minimum,
                        maximum: modal.maximum,
                        distinct_targets: modal.distinct_targets,
                        allow_repeat: modal.allow_repeat,
                        modes,
                    },
                )
            }
            RuleFrame::TriggerTarget(frame) if frame.ordered_any_target_bounds().is_some() => {
                let (min, max) = frame
                    .ordered_any_target_bounds()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let mut limit = max.min(frame.domain().len());
                let secondary_targets = if let Some(groups) = frame.secondary_targets() {
                    limit = if groups.values().any(|choices| !choices.is_empty()) {
                        2
                    } else {
                        1
                    };
                    groups
                        .iter()
                        .map(|(primary, targets)| {
                            Ok(crate::player_wire::PlayerSecondaryTargets {
                                primary: self.target(*primary)?,
                                targets: targets
                                    .iter()
                                    .map(|target| self.target(*target))
                                    .collect::<Result<_, _>>()?,
                            })
                        })
                        .collect::<Result<Vec<_>, NativeProjectionError>>()?
                } else {
                    Vec::new()
                };
                (
                    DecisionKind::TriggerTarget,
                    PlayerDomain::TargetList {
                        targets: frame
                            .domain()
                            .iter()
                            .map(|target| self.target(*target))
                            .collect::<Result<_, _>>()?,
                        minimum: count(min)?,
                        maximum: count(limit)?,
                        secondary_targets,
                    },
                )
            }
            RuleFrame::TriggerTarget(frame) if frame.permanent_target_bounds().is_some() => {
                let (min, max) = frame
                    .permanent_target_bounds()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let ids = frame
                    .domain()
                    .iter()
                    .map(|target| match target {
                        TargetRef::Object(object) => Ok(*object),
                        _ => Err(NativeProjectionError::MissingNativeValue),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (
                    DecisionKind::TriggerTarget,
                    self.objects_range(&ids, min, max.min(ids.len()))?,
                )
            }
            RuleFrame::TriggerTarget(frame) => {
                let targets = frame
                    .domain()
                    .iter()
                    .map(|target| self.target(*target))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::TriggerTarget,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::TokenAuraChoice(frame) => {
                let targets = frame
                    .domain()
                    .iter()
                    .map(|target| self.target(*target))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::LinkedAuraChoice,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::LinkedAuraChoice(frame) => {
                let targets = frame
                    .domain()
                    .iter()
                    .map(|target| self.target(*target))
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::LinkedAuraChoice,
                    PlayerDomain::Targets { targets },
                )
            }
            RuleFrame::Combat(CombatDecision::Attackers(frame)) if frame.payment().is_some() => {
                let cost = frame
                    .payment()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let choices: Vec<_> = self.scope.source.attack_mana_choices().cloned().collect();
                let activations = choices
                    .into_iter()
                    .map(|choice| {
                        let source = self
                            .visible
                            .contains(&choice.source)
                            .then(|| self.visible_object(choice.source))
                            .transpose()?;
                        Ok(crate::player_wire::PlayerAttackMana {
                            id: choice.id.0,
                            source,
                            reversible: choice.reversible,
                            depends_on: choice.dependencies.iter().map(|id| id.0).collect(),
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::DeclareAttackers,
                    PlayerDomain::AttackCost {
                        generic: quantity(&cost.generic)?,
                        white_phyrexian: quantity(&cost.white_phyrexian)?,
                        activations,
                    },
                )
            }
            RuleFrame::Combat(CombatDecision::Attackers(frame)) => {
                let defenders: Vec<_> = frame
                    .defenders()
                    .iter()
                    .copied()
                    .map(TargetRef::Player)
                    .chain(
                        frame
                            .planeswalkers()
                            .iter()
                            .chain(frame.battles())
                            .copied()
                            .map(TargetRef::Object),
                    )
                    .collect();
                let choices = frame
                    .eligible_attackers()
                    .iter()
                    .map(|id| {
                        Ok(AttackerDomain {
                            banding: frame.banding_attackers().contains(id),
                            band_qualities: frame.band_qualities(*id).map(band_quality).collect(),
                            bands_with_other: frame
                                .bands_with_other(*id)
                                .map(band_quality)
                                .collect(),
                            attacker: self.visible_object(*id)?,
                            defenders: defenders
                                .iter()
                                .filter(|t| {
                                    !frame
                                        .forbidden_defenders()
                                        .get(id)
                                        .is_some_and(|domain| domain.contains(t))
                                        && frame
                                            .required_attacks()
                                            .get(id)
                                            .is_none_or(|domain| domain.contains(t))
                                })
                                .map(|target| self.target(*target))
                                .collect::<Result<Vec<_>, _>>()?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                let choices: Vec<AttackerDomain> = choices;
                let mut prices = Vec::new();
                for (&defender, cost) in frame.defender_costs() {
                    let defender = self.target(defender)?;
                    if choices
                        .iter()
                        .any(|entry| entry.defenders.contains(&defender))
                    {
                        prices.push(crate::player_wire::PlayerAttackPrice {
                            defender,
                            generic: quantity(&cost.generic)?,
                            white_phyrexian: quantity(&cost.white_phyrexian)?,
                        });
                    }
                }
                (
                    DecisionKind::DeclareAttackers,
                    PlayerDomain::Attackers {
                        prices,
                        capped_defenders: frame
                            .capped_defenders()
                            .iter()
                            .map(|&id| self.app_seat(id))
                            .collect::<Result<_, _>>()?,
                        single_attacker: frame.single_attacker(),
                        choices,
                        required: frame
                            .required_attacks()
                            .keys()
                            .map(|id| self.visible_object(*id))
                            .collect::<Result<_, _>>()?,
                    },
                )
            }
            RuleFrame::Combat(CombatDecision::Blockers(frame)) => {
                let minimum_if_blocked = frame
                    .group_minimums()
                    .map(|(attacker, minimum)| {
                        Ok(BlockMinimum {
                            attacker: self.visible_object(attacker)?,
                            minimum: count(minimum)?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                let choices = frame
                    .eligible_blockers()
                    .iter()
                    .map(|id| {
                        Ok(BlockerDomain {
                            blocker: self.visible_object(*id)?,
                            attackers: frame
                                .legal_attackers(*id)
                                .iter()
                                .map(|id| self.visible_object(*id))
                                .collect::<Result<_, _>>()?,
                            minimum: count(0)?,
                            maximum: count(
                                frame
                                    .maximum_blocks(*id)
                                    .min(frame.legal_attackers(*id).len()),
                            )?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::DeclareBlockers,
                    PlayerDomain::Blockers {
                        defending_seat_id: (frame.defending_player() != frame.chooser())
                            .then(|| self.app_seat(frame.defending_player()))
                            .transpose()?,
                        single_blocker: frame.single_blocker(),
                        choices,
                        minimum_if_blocked,
                        required: frame
                            .mandatory_blockers()
                            .map(|id| self.visible_object(id))
                            .collect::<Result<_, _>>()?,
                    },
                )
            }
            RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) => {
                let sources = frame
                    .domains()
                    .iter()
                    .map(|domain| {
                        Ok(CombatDamageDomain {
                            source: self.visible_object(domain.source())?,
                            targets: domain
                                .recipients()
                                .iter()
                                .map(|target| self.recipient(*target))
                                .collect::<Result<_, _>>()?,
                            total: quantity(domain.power())?,
                            trample_lethal: domain
                                .trample_lethal()
                                .iter()
                                .map(|(&blocker, amount)| {
                                    Ok(TrampleMinimum {
                                        blocker: self.visible_object(blocker)?,
                                        amount: quantity(amount)?,
                                    })
                                })
                                .collect::<Result<_, NativeProjectionError>>()?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::AssignCombatDamage,
                    PlayerDomain::CombatDamage { sources },
                )
            }
            RuleFrame::Opening(OpeningDecision::Declare { can_mulligan, .. }) => {
                let mut options = vec![self.option(
                    NativeReference::Mulligan(MulliganChoice::Keep),
                    "Keep".into(),
                )?];
                if *can_mulligan {
                    options.push(self.option(
                        NativeReference::Mulligan(MulliganChoice::Mulligan),
                        "Mulligan".into(),
                    )?);
                }
                (
                    DecisionKind::Mulligan,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::Opening(OpeningDecision::Bottom {
                count: required,
                hand,
                ..
            }) => (DecisionKind::Bottom, self.objects_domain(hand, *required)?),
            RuleFrame::Opening(OpeningDecision::PregameBattlefield { eligible, .. }) => (
                DecisionKind::SelectResolution,
                self.objects_range(eligible, 0, eligible.len())?,
            ),
            RuleFrame::CopyTargets(frame) => {
                let number = frame.target_ordinal() + 1;
                let ordering = false;
                let mut options = vec![];
                if self.copy_probe(CopyTargetChoice::Retain)? {
                    options.push(self.option(
                        NativeReference::CopyTargets(CopyTargetChoice::Retain),
                        format!("Keep original target {number}"),
                    )?);
                }
                let targets: Vec<_> = self
                    .visible
                    .iter()
                    .copied()
                    .map(TargetRef::Object)
                    .chain(
                        self.scope
                            .binding
                            .seats
                            .iter()
                            .map(|entry| TargetRef::Player(entry.native_seat)),
                    )
                    .chain(
                        source
                            .execution()
                            .window()
                            .entries()
                            .iter()
                            .chain(source.execution().window().resolving())
                            .map(|entry| TargetRef::Stack(entry.id())),
                    )
                    .collect();
                for target in targets {
                    let choice = CopyTargetChoice::Change(target);
                    if self.copy_probe(choice)? {
                        let name = self.target_label(target)?;
                        let label = if ordering {
                            format!("Put the copy targeting {name} on the stack next")
                        } else {
                            format!("Change target {number} to {name}")
                        };
                        options.push(self.option(NativeReference::CopyTargets(choice), label)?);
                    }
                }
                if options.is_empty() {
                    return Err(NativeProjectionError::MissingNativeValue);
                }
                (
                    DecisionKind::CopyTargets,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::Damage(DamageDecision::Order(frame)) => {
                let mut options = vec![];
                for application in frame.applications() {
                    let label = match application.effect {
                        DamageEffectId::UntilEndOfTurn { .. } => {
                            "Apply this turn’s damage prevention".into()
                        }
                        DamageEffectId::EmblemCap {
                            source: damage_source,
                            ..
                        } => {
                            let component = source
                                .execution()
                                .pending_damage()
                                .and_then(|batch| {
                                    batch.components().find(|component| {
                                        component.attempt().source.id() == damage_source
                                    })
                                })
                                .ok_or(NativeProjectionError::MissingNativeValue)?;
                            ensure_public_damage_source(
                                &component.attempt().source,
                                self.scope.binding.actor,
                            )?;
                            let name = match &component.attempt().source {
                                spellwake_engine::damage::DamageSource::Object {
                                    object, ..
                                } if object.face_down().is_some() => {
                                    "a face-down source".to_owned()
                                }
                                spellwake_engine::damage::DamageSource::Object {
                                    object, ..
                                } => {
                                    let program = source
                                        .execution()
                                        .copiable_program(object.id())
                                        .or_else(|| {
                                            object.card_id().and_then(|id| {
                                                source
                                                    .execution()
                                                    .programs()
                                                    .find(|(key, _)| *key == id)
                                                    .map(|(_, program)| program)
                                            })
                                        });
                                    if let Some(program) = program {
                                        self.display.object(object, program)?.characteristics.name
                                    } else {
                                        "a source".to_owned()
                                    }
                                }
                                spellwake_engine::damage::DamageSource::SpellCopy { .. } => {
                                    "a spell copy".to_owned()
                                }
                                spellwake_engine::damage::DamageSource::Emblem { .. } => {
                                    "an emblem".to_owned()
                                }
                            };
                            format!("Prevent all but 1 damage from {name}")
                        }
                        DamageEffectId::RemovePlusOneToPreventDamage(_, _) => {
                            "Prevent the damage and remove a +1/+1 counter".into()
                        }
                        DamageEffectId::ShieldCounter(_) => {
                            "Prevent the damage and remove a shield counter".into()
                        }
                        DamageEffectId::CombatDamageToMill(_, _) => {
                            "Mill cards instead of taking this combat damage".into()
                        }
                        DamageEffectId::StaticPreventionWithExile(_, _) => {
                            "Prevent the damage and exile cards from the library".into()
                        }
                        DamageEffectId::StaticPrevention(_) => "Apply damage prevention".into(),
                        DamageEffectId::StaticDouble(_)
                        | DamageEffectId::DoubleDamageFromEquippedCreature(_)
                        | DamageEffectId::DoubleDamageToEquippedCreature(_) => {
                            "Double the damage".into()
                        }
                        DamageEffectId::StaticModify(_, _) => {
                            "Apply static damage replacement".into()
                        }
                        DamageEffectId::TemporaryStaticModify(_) => {
                            "Apply temporary damage replacement".into()
                        }
                        DamageEffectId::TemporaryPrevention(_) => {
                            "Prevent damage with temporary shield".into()
                        }
                        DamageEffectId::StaticHalfPrevention { .. } => {
                            "Prevent half the damage, rounded up".into()
                        }
                        DamageEffectId::ProtectionSubtype(_)
                        | DamageEffectId::ProtectionFrom(_) => {
                            "Prevent damage from protected sources".into()
                        }
                        DamageEffectId::ProtectionMulticolored => {
                            "Prevent damage from multicolored sources".into()
                        }
                        DamageEffectId::Protection(color) => format!(
                            "Prevent damage from {} sources",
                            format!("{color:?}").to_lowercase()
                        ),
                        DamageEffectId::Prevention(id) => {
                            let effect = source
                                .execution()
                                .prevention_shield(id)
                                .ok_or(NativeProjectionError::MissingNativeValue)?;
                            format!(
                                "Prevent up to {} damage",
                                String::from(effect.remaining.clone())
                            )
                        }
                        DamageEffectId::Multiplier(id) => {
                            let effect = source
                                .execution()
                                .damage_multiplier(id)
                                .ok_or(NativeProjectionError::MissingNativeValue)?;
                            format!("Multiply damage by {}", String::from(effect.factor.clone()))
                        }
                    };
                    options.push(
                        self.option(NativeReference::DamageApplication(*application), label)?,
                    );
                }
                (
                    DecisionKind::DamageOrder,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::Damage(DamageDecision::Allocation(frame)) => {
                let attempts = frame
                    .eligible()
                    .iter()
                    .map(|attempt| {
                        ensure_public_damage_source(attempt.source(), self.scope.binding.actor)?;
                        let face_down = matches!(attempt.source(),spellwake_engine::damage::DamageSource::Object {object,..} if object.face_down().is_some());
                        let mut label = if face_down { format!("{} damage", String::from(attempt.amount().clone())) }
                            else { self.display.prevention_attempt(attempt)? };
                        if matches!(frame.application().scope, spellwake_engine::damage::DamageScope::ControlledRecipients(_)) {
                            use spellwake_engine::damage::DamageRecipient;
                            let recipient = match attempt.target() {
                                DamageRecipient::Player(player) => TargetRef::Player(player),
                                DamageRecipient::Creature(object) | DamageRecipient::Planeswalker(object) | DamageRecipient::Battle(object) => TargetRef::Object(object),
                            };
                            label = format!("{label} to {}", self.target_label(recipient)?);
                        }
                        Ok(PreventionDomain {
                            attempt: self
                                .issue(NativeReference::DamageAttempt(attempt.attempt()))?,
                            maximum: quantity(attempt.amount())?,
                            label,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::AllocatePrevention,
                    PlayerDomain::Prevention {
                        attempts,
                        required: quantity(frame.required())?,
                    },
                )
            }
            RuleFrame::ManaColor(frame) => {
                let options = frame
                    .colors()
                    .iter()
                    .map(|kind| {
                        self.option(
                            NativeReference::ManaColor(*kind),
                            mana_label(*kind).to_owned(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::ManaColor,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::EntryChoice(frame) => self.entry_choice_domain(
                frame.rule(),
                frame.eligible_reveals(),
                frame.card_names(),
            )?,
            RuleFrame::TokenEntryChoice(frame) => self.entry_choice_domain(
                frame.rule(),
                frame.eligible_reveals(),
                frame.card_names(),
            )?,
            RuleFrame::CastRecovery(frame) => {
                let cost = frame.mana_cost();
                let minimum_spend = cost
                    .required
                    .iter()
                    .filter(|(_, amount)| amount.is_positive())
                    .map(|(kind, amount)| {
                        Ok(ManaSpend {
                            color: mana(*kind),
                            amount: quantity(amount)?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                let maximum_spend = ManaType::ALL
                    .into_iter()
                    .map(|kind| {
                        Ok(ManaSpend {
                            color: mana(kind),
                            amount: quantity(&cost.total())?,
                        })
                    })
                    .collect::<Result<_, NativeProjectionError>>()?;
                (
                    DecisionKind::CastRecovery,
                    PlayerDomain::Payment {
                        minimum_spend,
                        maximum_spend,
                        total: quantity(&cost.total())?,
                        allow_decline: true,
                        up_to: false,
                    },
                )
            }
            RuleFrame::Proliferate(frame) => (
                DecisionKind::Proliferate,
                PlayerDomain::TargetList {
                    secondary_targets: vec![],
                    targets: frame
                        .domain()
                        .keys()
                        .map(|who| self.target(*who))
                        .collect::<Result<_, _>>()?,
                    minimum: count(0)?,
                    maximum: count(frame.domain().len())?,
                },
            ),
            RuleFrame::ResolutionManaColor(frame) if frame.split().is_some() => {
                let (first, second, total) = frame
                    .split()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                (
                    DecisionKind::Number,
                    PlayerDomain::Number {
                        minimum: 0.into(),
                        maximum: total.clone(),
                        mana_split: Some([mana(first), mana(second)]),
                    },
                )
            }
            RuleFrame::ResolutionManaColor(frame) => {
                let options = frame
                    .colors()
                    .iter()
                    .map(|color| {
                        self.option(
                            NativeReference::ManaColor(*color),
                            mana_label(*color).to_owned(),
                        )
                    })
                    .collect::<Result<_, _>>()?;
                (
                    DecisionKind::ManaColor,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::PromptChoice(frame) => {
                use crate::player_wire::{PlayerPromptCategory, PlayerPromptDomain};
                use spellwake_engine::toolbox::{ChoiceCategory, ResolvedPromptDomain};
                let category = match frame.category {
                    ChoiceCategory::Generic => PlayerPromptCategory::Generic,
                    ChoiceCategory::Modal => PlayerPromptCategory::Modal,
                    ChoiceCategory::VillainousChoice => PlayerPromptCategory::VillainousChoice,
                    ChoiceCategory::Dilemma => PlayerPromptCategory::Dilemma,
                    ChoiceCategory::Vote => PlayerPromptCategory::Vote,
                    ChoiceCategory::PileSplit => PlayerPromptCategory::PileSplit,
                    ChoiceCategory::Search => PlayerPromptCategory::Search,
                    ChoiceCategory::Tribute => PlayerPromptCategory::Tribute,
                    ChoiceCategory::Clash => PlayerPromptCategory::Clash,
                    ChoiceCategory::Custom(_) => PlayerPromptCategory::Custom,
                };
                let prompt = match &frame.domain {
                    ResolvedPromptDomain::Objects {
                        candidates,
                        min,
                        max,
                        hidden,
                    } => {
                        let handles = candidates
                            .iter()
                            .map(|id| self.visible_object(*id))
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Objects {
                            candidates: handles,
                            minimum: count(*min)?,
                            maximum: count(*max)?,
                            hidden: *hidden,
                        }
                    }
                    ResolvedPromptDomain::Players {
                        candidates,
                        min,
                        max,
                    } => {
                        let seats = candidates
                            .iter()
                            .map(|seat| self.app_seat(*seat))
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Players {
                            seats,
                            minimum: *min,
                            maximum: *max,
                        }
                    }
                    ResolvedPromptDomain::Options {
                        labels,
                        min,
                        max,
                        allow_repeat,
                    } => {
                        let options = labels
                            .iter()
                            .enumerate()
                            .map(|(idx, label)| {
                                let i = u8::try_from(idx)
                                    .map_err(|_| NativeProjectionError::MissingNativeValue)?;
                                self.option(NativeReference::OptionIndex(i), label.clone())
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Options {
                            options,
                            minimum: *min,
                            maximum: *max,
                            allow_repeat: *allow_repeat,
                        }
                    }
                    ResolvedPromptDomain::Number { min, max } => PlayerPromptDomain::Number {
                        minimum: min.clone(),
                        maximum: max.clone(),
                    },
                    ResolvedPromptDomain::Color { allowed } => {
                        let options = allowed
                            .iter()
                            .map(|c| {
                                self.option(
                                    NativeReference::ManaColor(*c),
                                    mana_label(*c).to_owned(),
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Options {
                            options,
                            minimum: 1,
                            maximum: 1,
                            allow_repeat: false,
                        }
                    }
                    ResolvedPromptDomain::CardType { allowed } => {
                        let options = allowed
                            .iter()
                            .enumerate()
                            .map(|(idx, ty)| {
                                let i = u8::try_from(idx)
                                    .map_err(|_| NativeProjectionError::MissingNativeValue)?;
                                self.option(NativeReference::OptionIndex(i), format!("{ty:?}"))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Options {
                            options,
                            minimum: 1,
                            maximum: 1,
                            allow_repeat: false,
                        }
                    }
                    ResolvedPromptDomain::CreatureType { allowed } => {
                        let options = allowed
                            .iter()
                            .map(|ct| {
                                let label = self
                                    .scope
                                    .source
                                    .execution()
                                    .creature_vocabulary()
                                    .label(*ct)
                                    .unwrap_or("Creature")
                                    .to_owned();
                                self.option(NativeReference::CreatureType(*ct), label)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Options {
                            options,
                            minimum: 1,
                            maximum: 1,
                            allow_repeat: false,
                        }
                    }
                    ResolvedPromptDomain::BasicLandType { allowed } => {
                        let options = allowed
                            .iter()
                            .enumerate()
                            .map(|(idx, lt)| {
                                let i = u8::try_from(idx)
                                    .map_err(|_| NativeProjectionError::MissingNativeValue)?;
                                self.option(NativeReference::OptionIndex(i), format!("{lt:?}"))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Options {
                            options,
                            minimum: 1,
                            maximum: 1,
                            allow_repeat: false,
                        }
                    }
                    ResolvedPromptDomain::PlaneswalkerType { allowed } => {
                        let options = allowed
                            .iter()
                            .map(|pw| {
                                self.option(
                                    NativeReference::PlaneswalkerType(*pw),
                                    pw.label().to_owned(),
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Options {
                            options,
                            minimum: 1,
                            maximum: 1,
                            allow_repeat: false,
                        }
                    }
                    ResolvedPromptDomain::CardName { nonland_only } => {
                        PlayerPromptDomain::CardName {
                            nonland_only: *nonland_only,
                        }
                    }
                    ResolvedPromptDomain::Partition {
                        candidates,
                        pile_count,
                        min_per_pile,
                    } => {
                        let handles = candidates
                            .iter()
                            .map(|id| self.visible_object(*id))
                            .collect::<Result<Vec<_>, _>>()?;
                        PlayerPromptDomain::Partition {
                            candidates: handles,
                            pile_count: *pile_count,
                            min_per_pile: *min_per_pile,
                        }
                    }
                };
                (
                    DecisionKind::PromptChoice,
                    PlayerDomain::PromptChoice { category, prompt },
                )
            }
            RuleFrame::OptionIndex(frame) => {
                let options = (0..frame.count())
                    .map(|idx| {
                        let label = frame
                            .labels()
                            .get(usize::from(idx))
                            .cloned()
                            .unwrap_or_else(|| format!("Option {}", usize::from(idx) + 1));
                        self.option(NativeReference::OptionIndex(idx), label)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (
                    DecisionKind::Modes,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
            RuleFrame::CreatureType(frame) => {
                let vocabulary = source.execution().creature_vocabulary();
                if vocabulary.id() != frame.vocabulary() {
                    return Err(NativeProjectionError::MissingNativeValue);
                }
                let mut options = vec![];
                for label in vocabulary.members() {
                    let id = vocabulary
                        .resolve_label(label)
                        .ok_or(NativeProjectionError::MissingNativeValue)?;
                    if !frame.excluding().contains(&id) {
                        options
                            .push(self.option(NativeReference::CreatureType(id), label.clone())?);
                    }
                }
                (
                    DecisionKind::CreatureType,
                    PlayerDomain::Options {
                        options,
                        query: None,
                    },
                )
            }
        };
        let subject_label = match pending.frame() {
            RuleFrame::ResolutionGraveyardOrder(frame)
                if frame.destination()
                    == spellwake_engine::program::PublicZoneDestination::Library =>
            {
                Some("Order these cards from top to bottom beneath your library.".into())
            }
            RuleFrame::CreatureType(frame) if !frame.previously_named().is_empty() => {
                let vocabulary = source.execution().creature_vocabulary();
                let named = frame
                    .previously_named()
                    .iter()
                    .map(|kind| {
                        vocabulary
                            .label(*kind)
                            .ok_or(NativeProjectionError::MissingNativeValue)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Some(format!("Previously chosen {}", named.join(", ")))
            }
            RuleFrame::DrawReplacement(frame) => Some(
                match frame.domain {
                    spellwake_engine::effects::DrawReplacementDomain::Miracle { .. } => {
                        "You may reveal the first card you draw this turn."
                    }
                    spellwake_engine::effects::DrawReplacementDomain::LandKind => {
                        "Draw normally or reveal cards until you find the chosen kind."
                    }
                    spellwake_engine::effects::DrawReplacementDomain::Collector(_) => {
                        "Choose the opponent who will share this draw."
                    }
                    spellwake_engine::effects::DrawReplacementDomain::Bottom(_) => {
                        "Order the revealed cards from top to bottom on the bottom of your library."
                    }
                }
                .into(),
            ),
            RuleFrame::CounterReplacement(_) => Some(
                "Choose how many doubling effects apply before adding the extra +1/+1 counter"
                    .to_owned(),
            ),
            RuleFrame::Mutation(_) => {
                Some("Choose 1 to put the mutating spell on top, or 2 to put it underneath.".into())
            }
            RuleFrame::LibraryTop(frame) if frame.is_reveal_only() => {
                Some("You may reveal the top card of your library.".to_owned())
            }
            RuleFrame::LibraryTop(frame) if frame.chooses_land_kind() => {
                Some("Choose 1 for land or 2 for nonland.".to_owned())
            }
            RuleFrame::RevealedPiles(frame) => Some(match frame.choice() {
                spellwake_engine::effects::PileChoice::Divide(_) => {
                    "Choose cards for pile 1. The remaining cards form pile 2.".to_owned()
                }
                spellwake_engine::effects::PileChoice::BottomOrder(_) => {
                    "Order the other pile from top to bottom on the bottom of your library."
                        .to_owned()
                }
                spellwake_engine::effects::PileChoice::GraveyardOrder(_) => {
                    "Order the cards from top to bottom in your graveyard.".to_owned()
                }
                spellwake_engine::effects::PileChoice::Take { first, second } => {
                    let names = |ids: &[ObjectId]| -> Result<String, NativeProjectionError> {
                        if ids.is_empty() {
                            Ok("empty".to_owned())
                        } else {
                            Ok(ids
                                .iter()
                                .map(|&id| self.revealed_library_card_name(id))
                                .collect::<Result<Vec<_>, _>>()?
                                .join(", "))
                        }
                    };
                    format!(
                        "Choose a pile for your hand. Pile 1 contains {}. Pile 2 contains {}.",
                        names(first)?,
                        names(second)?
                    )
                }
            }),
            RuleFrame::DamageSourceChoice(_) => Some("Choose a source of damage.".into()),
            RuleFrame::ReplacementChooser(_) => {
                Some("Choose a player to make the decision for the player who left.".into())
            }
            RuleFrame::Checks(CheckDecision::BattleProtector(frame)) => {
                let _ = self.visible_object(frame.battle())?;
                Some("Choose a new protector for the Siege.".into())
            }
            RuleFrame::EntryChoice(frame) => {
                let hidden_search = match self.visible_object(frame.object()) {
                    Ok(_) => false,
                    Err(NativeProjectionError::HiddenReference)
                        if source
                            .execution()
                            .pending_search_entry_subjects()
                            .is_some_and(|(chooser, subjects)| {
                                chooser == self.scope.binding.actor
                                    && subjects.contains(&frame.object())
                            }) =>
                    {
                        true
                    }
                    Err(error) => return Err(error),
                };
                let object = source
                    .execution()
                    .window()
                    .cards()
                    .get(frame.object())
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let program = if hidden_search {
                    source.execution().base_program(frame.object())
                } else {
                    source.execution().copiable_program(frame.object())
                }
                .ok_or(NativeProjectionError::MissingNativeValue)?;
                Some(self.display.object(object, program)?.characteristics.name)
            }
            RuleFrame::LinkedReturnOrder(frame) => Some(match frame.destination() {
                spellwake_engine::program::PublicZoneDestination::Battlefield => {
                    "Order returning permanents from earliest to latest".into()
                }
                spellwake_engine::program::PublicZoneDestination::Graveyard => {
                    "Order cards in the graveyard from top to bottom".into()
                }
                spellwake_engine::program::PublicZoneDestination::Library => {
                    "Order cards from top to bottom".into()
                }
                spellwake_engine::program::PublicZoneDestination::Hand => {
                    "Order cards returning to hand".into()
                }
                spellwake_engine::program::PublicZoneDestination::Exile => {
                    "Order exiled cards".into()
                }
                spellwake_engine::program::PublicZoneDestination::Command => {
                    "Order cards moving to the command zone".into()
                }
            }),
            RuleFrame::CastRecovery(frame) => Some(if frame.life_cost().is_positive() {
                format!(
                    "Pay {} life to complete the cast, or abandon it",
                    String::from(frame.life_cost().clone())
                )
            } else {
                "Complete the cast or abandon it".into()
            }),
            RuleFrame::TokenEntryChoice(frame) => {
                Some(self.display.token_preview(frame.preview())?)
            }
            RuleFrame::OptionalEffect(frame) if frame.token_offer().is_some() => {
                let (amount, replies) = frame
                    .token_offer()
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let mut label = format!("Create {} tokens?", String::from(amount.clone()));
                for (player, accepted) in replies {
                    let result = if *accepted { "accepted" } else { "declined" };
                    label.push_str(&format!(" Seat {} {result}.", u64::from(player.0) + 1));
                }
                Some(label)
            }
            RuleFrame::TriggerTarget(frame) if frame.divides_three_damage() => Some(
                "Divide 3 damage among your targets. With 2 targets, the first takes 2 damage."
                    .into(),
            ),
            RuleFrame::Combat(CombatDecision::Blockers(frame))
                if frame.has_satisfiable_block_requirements() =>
            {
                Some("Satisfy as many blocking requirements as possible.".into())
            }
            RuleFrame::TokenAuraChoice(frame) => Some(self.display.token_preview(frame.preview())?),
            RuleFrame::LinkedAuraChoice(frame) => {
                self.visible_object(frame.object())?;
                let object = source
                    .execution()
                    .window()
                    .cards()
                    .get(frame.object())
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                let program = source
                    .execution()
                    .copiable_program(frame.object())
                    .ok_or(NativeProjectionError::MissingNativeValue)?;
                Some(self.display.object(object, program)?.characteristics.name)
            }
            RuleFrame::UntapKeepTapped(_) => {
                Some("Choose permanents to keep tapped during your untap step.".into())
            }
            RuleFrame::Opening(OpeningDecision::PregameBattlefield { .. }) => {
                Some("Choose opening-hand cards to begin the game on the battlefield.".into())
            }
            _ => None,
        };
        Ok(vec![PlayerDecision {
            decision: self.issue(NativeReference::Decision(pending.id()))?,
            subject_label,
            basis: self.scope.basis().clone(),
            responding_seat_ids: vec![self.scope.viewer_seat_id()],
            visibility: DecisionVisibility::RespondingSeat,
            kind,
            domain,
            deadline_at: self.metadata.deadline_at.clone(),
        }])
    }
}

fn band_quality(
    quality: spellwake_engine::combat::BandQuality,
) -> crate::player_wire::PlayerBandQuality {
    use crate::player_wire::PlayerBandQuality;
    match quality {
        spellwake_engine::combat::BandQuality::LegendaryCreatures => {
            PlayerBandQuality::LegendaryCreatures
        }
        spellwake_engine::combat::BandQuality::CreaturesNamedWolvesOfTheHunt => {
            PlayerBandQuality::CreaturesNamedWolvesOfTheHunt
        }
    }
}
