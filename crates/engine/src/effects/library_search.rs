//! Qualified library searches (CR 701.23, 702.29e). The native root owns entropy.
use super::*;
use crate::{
    occurrence::{EventCause, RulesOccurrence, ZoneReason},
    program::{
        AuraSearchHost, LibrarySearch, SearchCardMatcher, SearchCount, SearchDestination,
        SearchExileLink, SearchManaValue, SearchPairConstraint, SearchSource, Supertype,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchEntryWork {
    pub(super) cursor: ResolutionCursor,
    pub(super) bound: LibraryTopFrame,
    pub(super) actor: SeatId,
    fixed_aura_host: Option<TargetRef>,
    pub(super) moves: Vec<(ObjectId, ObjectDestination)>,
    pub(super) tapped: bool,
    pub(super) colors: Vec<(ObjectId, crate::mana::ManaType)>,
    pub(super) reveals: Vec<(ObjectId, Option<ObjectId>)>,
    pub(super) chapters: Vec<(ObjectId, u8)>,
    pub(super) creature_types: Vec<(
        ObjectId,
        spellwake_contracts::creature_types::CreatureTypeId,
    )>,
    pub(super) entropy: EntropyTape,
    pub(super) batch: ZoneReplacementBatch,
    pub(super) reason: ZoneReason,
    pub(super) action: crate::program::ZoneActionKind,
    pub(super) graveyard_order: Option<Vec<ObjectId>>,
}
impl SearchEntryWork {
    pub(super) fn graveyard_frame(
        &self,
        revision: u64,
        window: &StackWindow,
    ) -> Result<Option<LinkedReturnOrderFrame>, EffectError> {
        let objects = self
            .moves
            .iter()
            .filter_map(|(id, dest)| {
                (matches!(dest, ObjectDestination::Battlefield { .. })
                    && self.batch.destination(*id).ok()
                        == Some(crate::program::PublicZoneDestination::Graveyard))
                .then_some(*id)
            })
            .collect::<Vec<_>>();
        if objects.len() > 1 && self.graveyard_order.is_none() {
            let chooser = window
                .cards()
                .get(objects[0])
                .ok_or(EffectError::UnknownObject)?
                .owner();
            return Ok(Some(LinkedReturnOrderFrame {
                revision,
                chooser,
                objects,
                destination: crate::program::PublicZoneDestination::Graveyard,
            }));
        }
        Ok(None)
    }
}

impl EffectWindow {
    fn object_names_for_search(
        &self,
        entry: &crate::stack_window::StackEntry,
        reference: crate::program::MoveSubjectRef,
        evaluated: &crate::characteristics::EvaluatedCharacteristics,
    ) -> Result<Option<spellwake_contracts::names::ObjectNames>, EffectError> {
        use crate::program::MoveSubjectRef;
        let obj_opt = match reference {
            MoveSubjectRef::AbilitySource => match entry.kind() {
                StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                    Some(source.id())
                }
                StackKind::CardSpell(id) => Some(*id),
                _ => None,
            },
            MoveSubjectRef::TriggeringObject => {
                if let Some(binding) = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    && let Some(cause) = binding.causes.members().first()
                {
                    if let Some(id) = cause.subject_object_id()
                        && let Some(view) = evaluated.get(id)
                    {
                        return Ok(Some(view.current_names.clone()));
                    }
                    if let crate::occurrence::TriggerRelated::ZoneChange { before, .. } =
                        &cause.related
                    {
                        return Ok(Some(before.current_names.clone()));
                    }
                    if let crate::occurrence::TriggerRelated::TokenCreated { after, .. } =
                        &cause.related
                    {
                        return Ok(Some(after.current_names.clone()));
                    }
                    if let crate::occurrence::TriggerRelated::Discard { before, .. } =
                        &cause.related
                    {
                        return Ok(Some(before.current_names.clone()));
                    }
                    return Ok(None);
                }
                None
            }
            other => self.resolve_pass_c_move_subject(entry, other)?,
        };
        if let Some(obj) = obj_opt
            && let Some(view) = evaluated.get(obj)
        {
            return Ok(Some(view.current_names.clone()));
        }
        if reference == MoveSubjectRef::AbilitySource
            && let Some(binding) = self
                .execution_bindings(entry.id())
                .and_then(|b| b.trigger.as_ref())
        {
            return Ok(Some(binding.source.current_names.clone()));
        }
        Ok(None)
    }

    pub(crate) fn search_reveals_selected(&self) -> Result<bool, EffectError> {
        match self.current_instruction()? {
            Instruction::SearchLibrary { filter } => Ok(filter.reveal),
            _ => Err(EffectError::WrongLibraryTopFrame),
        }
    }
    pub(super) fn searched_curse_host(
        &self,
        host: AuraSearchHost,
    ) -> Result<Option<TargetRef>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        match host {
            AuraSearchHost::TargetPlayer => {
                let target = self.instruction_target(entry)?.choice().target;
                Ok(matches!(target, TargetRef::Player(_)).then_some(target))
            }
            AuraSearchHost::EnchantedPlayerWithDifferentCurseNames => {
                let StackKind::TriggeredAbility { source } = entry.kind() else {
                    return Err(EffectError::InconsistentProgram);
                };
                Ok(self
                    .attachment_host_for_ability(source.id())
                    .filter(|target| matches!(target, TargetRef::Player(_))))
            }
        }
    }

    pub(super) fn search_frame(
        &self,
        operation: &Instruction,
    ) -> Result<LibraryTopFrame, EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongLibraryTopFrame)?;
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongLibraryTopFrame)?;
        let player = match operation {
            Instruction::SearchLibrary { filter } => match filter.searcher {
                crate::program::ResolvingPlayer::Controller => entry.controller(),
                other => self.resolving_player(other)?,
            },
            _ => entry.controller(),
        };
        let (filter, other_library): (Option<&LibrarySearch>, Option<SeatId>) = match operation {
            Instruction::SearchLibrary { filter } => match filter.source {
                SearchSource::PlayerLibrary(subject) => (Some(filter), Some(subject)),
                SearchSource::TargetOpponentLibrary | SearchSource::TargetPlayerLibrary => {
                    let bound = self.instruction_target(entry)?;
                    let TargetRef::Player(subject) = bound.choice().target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    (Some(filter), Some(subject))
                }
                SearchSource::OwnLibrary | SearchSource::OwnLibraryOrGraveyard => {
                    (Some(filter), None)
                }
            },
            _ => return Err(EffectError::WrongLibraryTopFrame),
        };
        let library = self.window.cards().library(other_library.unwrap_or(player));
        let counter_mana_value = match filter.and_then(LibrarySearch::source_counter_kind) {
            Some(kind) => Some(self.source_counter_value_at_resolution(entry, kind)?),
            None => None,
        };
        let evaluated = self.evaluated_characteristics()?;
        let forbidden_curse_names = if filter.and_then(LibrarySearch::curse_host)
            == Some(AuraSearchHost::EnchantedPlayerWithDifferentCurseNames)
        {
            let host =
                self.searched_curse_host(AuraSearchHost::EnchantedPlayerWithDifferentCurseNames)?;
            evaluated
                .objects
                .values()
                .filter(|aura| {
                    host.is_some()
                        && aura.zone == ObjectZone::Battlefield
                        && aura.attached_to == host
                        && aura
                            .current_enchantment_subtypes
                            .contains(&crate::program::EnchantmentSubtype::Curse)
                })
                .map(|aura| &aura.current_names)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut eligible = Vec::new();
        let searches_graveyard =
            filter.is_some_and(|search| search.source == SearchSource::OwnLibraryOrGraveyard);
        let graveyard = if searches_graveyard {
            self.window.cards().graveyard(player)
        } else {
            &[]
        };
        for &id in library.iter().chain(graveyard) {
            let current = evaluated.get(id).ok_or(EffectError::InconsistentProgram)?;
            let Some(search) = filter else {
                eligible.push(id);
                continue;
            };
            let base_match = match &search.matcher {
                SearchCardMatcher::Any => true,
                SearchCardMatcher::Nonland => !current.has_type(CardType::Land),
                SearchCardMatcher::BasicLand => {
                    current.has_type(CardType::Land)
                        && current.current_supertypes.contains(&Supertype::Basic)
                }
                SearchCardMatcher::LandSubtypes {
                    subtypes,
                    require_land_type,
                } => {
                    (!require_land_type || current.has_type(CardType::Land))
                        && !current.current_land_subtypes.is_disjoint(subtypes)
                }
                SearchCardMatcher::CardTypes(types) => {
                    types.iter().any(|kind| current.has_type(*kind))
                }
                SearchCardMatcher::CreatureSubtype(kind) => {
                    current.current_creature_subtypes.contains(kind)
                }
                SearchCardMatcher::Equipment => {
                    current
                        .current_artifact_subtypes
                        .contains(&crate::program::ArtifactSubtype::Equipment)
                        || self
                            .copiable_program(id)
                            .and_then(Program::permanent)
                            .is_some_and(|permanent| {
                                permanent.attachment
                                    == Some(crate::attachment::AttachmentKind::Equipment)
                            })
                }
                SearchCardMatcher::CurseForHost(_) => {
                    current
                        .current_enchantment_subtypes
                        .contains(&crate::program::EnchantmentSubtype::Curse)
                        && !forbidden_curse_names
                            .iter()
                            .any(|names| self.name_rules.shares_name(names, &current.current_names))
                }
                SearchCardMatcher::AnyOfNames(names) => names
                    .iter()
                    .any(|name| self.name_rules.has_name(&current.current_names, name)),
                SearchCardMatcher::SameNameAs { reference, extra } => {
                    let ref_names = self.object_names_for_search(entry, *reference, &evaluated)?;
                    let name_ok = ref_names.as_ref().is_some_and(|names| {
                        self.name_rules.shares_name(names, &current.current_names)
                    });
                    let extra_ok = extra.as_deref().is_none_or(|obj_filter| {
                        let excluded = BTreeSet::new();
                        obj_filter.matches_static_characteristics(current, &excluded, || {
                            self.copiable_program(id).map(Program::mana_value)
                        })
                    });
                    name_ok && extra_ok
                }
                SearchCardMatcher::Filtered(obj_filter) => {
                    let excluded = BTreeSet::new();
                    obj_filter.matches_static_characteristics(current, &excluded, || {
                        self.copiable_program(id).map(Program::mana_value)
                    })
                }
            };
            let mv_match = match &search.mana_value {
                None => true,
                Some(SearchManaValue::Exact(value)) => {
                    let program = self
                        .copiable_program(id)
                        .ok_or(EffectError::InconsistentProgram)?;
                    program.mana_value() == *value
                }
                Some(SearchManaValue::AtLeast(minimum)) => self
                    .copiable_program(id)
                    .is_some_and(|program| program.mana_value() >= i64::from(*minimum).into()),
                Some(SearchManaValue::AtMost(maximum)) => self
                    .copiable_program(id)
                    .is_some_and(|program| program.mana_value() <= i64::from(*maximum).into()),
                Some(SearchManaValue::AtMostAnnouncedX) => {
                    let x = entry.chosen_x().cloned().unwrap_or_else(|| 0.into());
                    self.copiable_program(id)
                        .is_some_and(|program| program.mana_value() <= x)
                }
                Some(SearchManaValue::EqualToSourceCounters(_)) => self
                    .copiable_program(id)
                    .is_some_and(|program| Some(program.mana_value()) == counter_mana_value),
                Some(SearchManaValue::ExactManaCostOneOf(costs)) => {
                    self.copiable_program(id).is_some_and(|program| {
                        program.x_mana_symbols == 0
                            && program.hybrid_symbols.is_empty()
                            && !program.extra_cast_costs.iter().any(|cost| {
                                matches!(
                                    cost,
                                    crate::activated::ExtraActivationCost::PaySnowMana(_)
                                )
                            })
                            && program
                                .printed_mana_cost()
                                .is_some_and(|mc| costs.contains(mc))
                    })
                }
            };
            if base_match && mv_match {
                eligible.push(id);
            }
        }
        let choose_destination = filter.is_some_and(|search| {
            search.destination == SearchDestination::HandOrBattlefieldIfCreatureDied
        }) && self.creatures_died_this_turn.is_positive();
        let enters = choose_destination
            || filter.is_some_and(|search| {
                matches!(
                    search.destination,
                    SearchDestination::Battlefield { .. }
                        | SearchDestination::SplitBattlefieldTappedAndHand
                )
            });
        let mut entry_colors = BTreeMap::new();
        let mut entry_reveals = BTreeMap::new();
        if enters {
            let evaluated = self.evaluated_characteristics()?;
            for &id in &eligible {
                match self
                    .copiable_program(id)
                    .and_then(Program::permanent)
                    .and_then(|value| value.entry_tap)
                {
                    Some(
                        crate::land::EntryTap::AlwaysChooseColorExcept(excluded)
                        | crate::land::EntryTap::ChooseColorExcept(excluded),
                    ) => {
                        entry_colors.insert(
                            id,
                            crate::mana::ManaType::ALL
                                .into_iter()
                                .filter(|color| {
                                    *color != crate::mana::ManaType::Colourless
                                        && *color != excluded
                                })
                                .collect(),
                        );
                    }
                    Some(
                        crate::land::EntryTap::ChooseColor
                        | crate::land::EntryTap::AlwaysChooseColor,
                    ) => {
                        entry_colors.insert(
                            id,
                            crate::mana::ManaType::ALL
                                .into_iter()
                                .filter(|color| *color != crate::mana::ManaType::Colourless)
                                .collect(),
                        );
                    }
                    _ => {}
                }
                match self
                    .copiable_program(id)
                    .and_then(Program::permanent)
                    .and_then(|value| value.entry_tap)
                {
                    Some(crate::land::EntryTap::UnlessRevealHandLandType(a, b)) => {
                        let candidates = self
                            .window
                            .cards()
                            .iter()
                            .filter(|card| {
                                card.zone() == ObjectZone::Hand(player) && card.card_id().is_some()
                            })
                            .filter(|card| {
                                evaluated.get(card.id()).is_some_and(|value| {
                                    value.current_land_subtypes.contains(&a.land_type())
                                        || value.current_land_subtypes.contains(&b.land_type())
                                })
                            })
                            .map(GameObject::id)
                            .collect();
                        entry_reveals.insert(id, candidates);
                    }
                    Some(crate::land::EntryTap::UnlessRevealHandSubtype { subtype, .. }) => {
                        let candidates = self
                            .window
                            .cards()
                            .iter()
                            .filter(|card| {
                                card.zone() == ObjectZone::Hand(player) && card.card_id().is_some()
                            })
                            .filter(|card| {
                                evaluated.get(card.id()).is_some_and(|value| {
                                    value.current_creature_subtypes.contains(&subtype)
                                })
                            })
                            .map(GameObject::id)
                            .collect();
                        entry_reveals.insert(id, candidates);
                    }
                    _ => {}
                }
            }
        }
        Ok(LibraryTopFrame::search(
            self.revision,
            entry.id(),
            cursor.instruction,
            player,
            library.to_vec(),
            eligible,
            super::library_top::SearchFrameMode {
                exile_owner: other_library,
                split: if choose_destination {
                    Some(super::library_top::SearchSplitMode::OneWithChoiceOfDestination)
                } else if filter.is_some_and(|search| {
                    search.destination == SearchDestination::SplitBattlefieldTappedAndHand
                }) {
                    Some(super::library_top::SearchSplitMode::TwoWithBattlefieldFirst)
                } else {
                    None
                },
                hand_graveyard: filter.is_some_and(|search| {
                    search.destination == SearchDestination::SplitHandAndGraveyard
                }),
                battlefield_batch: filter.is_some_and(LibrarySearch::is_battlefield_batch),
                exile_batch: filter.is_some_and(LibrarySearch::is_exile_batch),
                hand_limit: match filter {
                    Some(search) if search.count == SearchCount::UpToPlayersWithMoreLands => Some(
                        self.rules_snapshot(None)?
                            .view()
                            .players_with_more_lands(player),
                    ),
                    Some(search) => match &search.count {
                        SearchCount::UpToExpr(expr)
                            if matches!(
                                search.destination,
                                SearchDestination::Hand
                                    | SearchDestination::Graveyard
                                    | SearchDestination::Exile { .. }
                            ) =>
                        {
                            Some(
                                self.resolve_value_expr(entry.controller(), expr)?
                                    .max(0.into())
                                    .to_usize()
                                    .unwrap_or(0),
                            )
                        }
                        _ => search.fixed_batch_limit(),
                    },
                    None => None,
                },
                shared_land_type: filter.is_some_and(|search| {
                    search.pair_constraint == Some(SearchPairConstraint::ShareLandType)
                }),
                forest_and_plains: filter.is_some_and(|search| {
                    matches!(
                        search.pair_constraint,
                        Some(SearchPairConstraint::DistinctLandTypes(_, _))
                    )
                }),
                required: filter.is_some_and(LibrarySearch::is_required) && !library.is_empty(),
                entry_colors,
                entry_reveals,
            },
        ))
    }

    /// Commit a selection and a host-driven shuffle in one candidate.
    // Public-boundary search resume signature kept flat for direct caller ergonomics.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resume_search(
        &self,
        expected: u64,
        bound: &LibraryTopFrame,
        actor: SeatId,
        selected: Option<ObjectId>,
        colors: &[(ObjectId, crate::mana::ManaType)],
        reveals: &[(ObjectId, Option<ObjectId>)],
        entropy: &EntropyTape,
    ) -> Result<(RuleProgress, Option<(EntropyTape, ShuffleRecord)>), EffectError> {
        let revision = self.next_revision(expected)?;
        if bound != &self.search_frame(self.current_instruction()?)? {
            return Err(EffectError::WrongLibraryTopFrame);
        }
        if bound.is_split_search()
            || bound.is_battlefield_batch_search()
            || bound.is_bounded_batch_search()
            || bound.is_hand_graveyard_search()
        {
            return Err(EffectError::WrongDecisionKind);
        }
        if actor != bound.chooser() {
            return Err(EffectError::WrongChooser);
        }
        if (selected.is_none() && bound.requires_search_result())
            || selected.is_some_and(|id| !bound.eligible_results().contains(&id))
        {
            return Err(EffectError::InvalidLibraryPartition);
        }
        if matches!(
            self.current_instruction()?,
            Instruction::SearchLibrary { filter }
                if filter.destination == SearchDestination::LibraryTop
        ) {
            if !colors.is_empty() || !reveals.is_empty() {
                return Err(EffectError::WrongDecisionKind);
            }
            let mut next = self.clone();
            next.revision = revision;
            let shuffle_before = next.rules_snapshot(None)?;
            let (window, entropy, shuffle) = next
                .window
                .search_shuffle_to_top(actor, selected, entropy)?;
            next.window = window;
            let mut journal = RulesJournal::default();
            next.emit_from(
                EventCause::Instruction {
                    entry: bound.entry(),
                    instruction: bound.instruction(),
                },
                &shuffle_before,
                None,
                &mut journal,
                |_, _, _| Ok(vec![RulesOccurrence::Shuffled(shuffle.clone())]),
            )?;
            if let Some(cursor) = next.resolution_cursor_mut() {
                cursor.results.last_moved_objects.clear();
                cursor.results.last_action_lki.clear();
                cursor.results.last_action_count = Some(0.into());
                cursor.results.last_action_succeeded = false;
            }
            next.advance_instruction()?;
            let progress = next.progress_rules(
                DamageJournal::default(),
                vec![],
                vec![],
                DrawJournal::default(),
                journal,
            )?;
            return Ok((progress, Some((entropy, shuffle))));
        }
        if let Instruction::SearchLibrary { filter } = self.current_instruction()? {
            if filter.is_exile_destination() {
                let moves = selected
                    .into_iter()
                    .map(|id| (id, ObjectDestination::Exile))
                    .collect();
                return self.commit_search_batch(
                    revision,
                    bound,
                    actor,
                    moves,
                    (false, colors, reveals),
                    entropy,
                );
            }
            if matches!(filter.destination, SearchDestination::Battlefield { .. }) {
                let moves = selected
                    .into_iter()
                    .map(|id| (id, ObjectDestination::Battlefield { controller: actor }))
                    .collect();
                return self.commit_search_batch(
                    revision,
                    bound,
                    actor,
                    moves,
                    (filter.enters_tapped(), colors, reveals),
                    entropy,
                );
            }
        }
        let (destination, reason) = if bound.search_exile_owner().is_some() {
            (ObjectDestination::Exile, ZoneReason::SearchToExile)
        } else if matches!(
            self.current_instruction()?,
            Instruction::SearchLibrary { filter }
                if filter.destination == SearchDestination::Graveyard
        ) {
            (ObjectDestination::Graveyard, ZoneReason::SearchToGraveyard)
        } else {
            (ObjectDestination::Hand, ZoneReason::SearchToHand)
        };
        let colors = self.search_entry_colors(&[], colors)?;
        let reveals = self.search_entry_reveals(bound, &[], reveals)?;
        let mut next = self.clone();
        next.revision = revision;
        let mut journal = RulesJournal::default();
        let mut moved_id = None;
        if let Some(id) = selected {
            let before = next.rules_snapshot(None)?;
            let original = next
                .window
                .cards()
                .get(id)
                .ok_or(EffectError::UnknownObject)?
                .clone();
            next.window =
                next.window
                    .record_resolution_move(next.window.revision(), id, destination)?;
            let card = original.card_id().ok_or(EffectError::InconsistentProgram)?;
            let current = next
                .window
                .cards()
                .current_identity(card)
                .and_then(|id| next.window.cards().get(id))
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            let entering = current.id();
            moved_id = Some(entering);
            if let Some(color) = colors.get(&id) {
                next.entry_color_choices.insert(entering, *color);
                if let Some(c) = color.color() {
                    next.characteristics.entry_color_choices.insert(entering, c);
                }
            }
            if let Some(reveal) = reveals.get(&id) {
                next.entry_choices
                    .insert(entering, crate::land::EntryChoice::Reveal(*reveal));
            }
            let current = next
                .window
                .cards()
                .get(entering)
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            next.emit_from(
                EventCause::Instruction {
                    entry: bound.entry(),
                    instruction: bound.instruction(),
                },
                &before,
                None,
                &mut journal,
                |previous, current_view, controllers| {
                    Ok(vec![zone_occurrence_at(
                        previous,
                        current_view,
                        &crate::object_identity::ObjectMovement {
                            before: original,
                            after: current,
                        },
                        reason,
                        controllers,
                    )?])
                },
            )?;
        }
        let shuffle_before = next.rules_snapshot(None)?;
        let (window, entropy, shuffle) = next
            .window
            .search_shuffle(bound.search_exile_owner().unwrap_or(actor), entropy)?;
        next.window = window;
        next.emit_from(
            EventCause::Instruction {
                entry: bound.entry(),
                instruction: bound.instruction(),
            },
            &shuffle_before,
            None,
            &mut journal,
            |_, _, _| Ok(vec![RulesOccurrence::Shuffled(shuffle.clone())]),
        )?;
        if let Some(cursor) = next.resolution_cursor_mut() {
            cursor.results.last_moved_objects = moved_id.into_iter().collect();
            cursor.results.last_action_lki.clear();
            cursor.results.last_action_count = Some(i64::from(moved_id.is_some()).into());
            cursor.results.last_action_succeeded = moved_id.is_some();
        }
        next.advance_instruction()?;
        let progress = next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            journal,
        )?;
        Ok((progress, Some((entropy, shuffle))))
    }
    /// Commit a split search, its simultaneous departures, and one host-driven shuffle.
    // Public-boundary split-search resume signature kept flat for direct caller ergonomics.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resume_split_search(
        &self,
        expected: u64,
        bound: &LibraryTopFrame,
        actor: SeatId,
        selection: (Option<ObjectId>, Option<ObjectId>),
        colors: &[(ObjectId, crate::mana::ManaType)],
        reveals: &[(ObjectId, Option<ObjectId>)],
        entropy: &EntropyTape,
    ) -> Result<(RuleProgress, Option<(EntropyTape, ShuffleRecord)>), EffectError> {
        let revision = self.next_revision(expected)?;
        let (battlefield, hand) = selection;
        if bound != &self.search_frame(self.current_instruction()?)? || !bound.is_split_search() {
            return Err(EffectError::WrongLibraryTopFrame);
        }
        if actor != bound.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let choose_destination = bound.offers_search_destination();
        if (if choose_destination {
            hand.is_some() && battlefield.is_some()
        } else {
            hand.is_some() && battlefield.is_none()
        }) || (battlefield.is_some() && battlefield == hand)
            || battlefield
                .into_iter()
                .chain(hand)
                .any(|id| !bound.eligible_results().contains(&id))
        {
            return Err(EffectError::InvalidLibraryPartition);
        }
        let mut moves = Vec::new();
        if let Some(id) = battlefield {
            moves.push((id, ObjectDestination::Battlefield { controller: actor }));
        }
        if let Some(id) = hand {
            moves.push((id, ObjectDestination::Hand));
        }
        self.commit_search_batch(
            revision,
            bound,
            actor,
            moves,
            (!choose_destination, colors, reveals),
            entropy,
        )
    }

    /// Assign the unrestricted searched cards to their exact destinations.
    pub(crate) fn resume_hand_graveyard_search(
        &self,
        expected: u64,
        bound: &LibraryTopFrame,
        actor: SeatId,
        hand: Option<ObjectId>,
        graveyard: Option<ObjectId>,
        entropy: &EntropyTape,
    ) -> Result<(RuleProgress, Option<(EntropyTape, ShuffleRecord)>), EffectError> {
        let revision = self.next_revision(expected)?;
        if bound != &self.search_frame(self.current_instruction()?)?
            || !bound.is_hand_graveyard_search()
        {
            return Err(EffectError::WrongLibraryTopFrame);
        }
        if actor != bound.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let count = usize::from(hand.is_some()) + usize::from(graveyard.is_some());
        if count != bound.eligible_results().len().min(2)
            || graveyard.is_some() && hand.is_none()
            || hand.is_some() && hand == graveyard
            || hand
                .into_iter()
                .chain(graveyard)
                .any(|id| !bound.eligible_results().contains(&id))
        {
            return Err(EffectError::InvalidLibraryPartition);
        }
        let mut moves = Vec::new();
        if let Some(id) = hand {
            moves.push((id, ObjectDestination::Hand));
        }
        if let Some(id) = graveyard {
            moves.push((id, ObjectDestination::Graveyard));
        }
        self.commit_search_batch(revision, bound, actor, moves, (false, &[], &[]), entropy)
    }

    /// Resolve a bounded search into hand or linked exile, with no battlefield entries.
    pub(crate) fn resume_bounded_batch_search(
        &self,
        expected: u64,
        bound: &LibraryTopFrame,
        actor: SeatId,
        selected: Vec<ObjectId>,
        entropy: &EntropyTape,
    ) -> Result<(RuleProgress, Option<(EntropyTape, ShuffleRecord)>), EffectError> {
        let revision = self.next_revision(expected)?;
        if bound != &self.search_frame(self.current_instruction()?)?
            || !bound.is_bounded_batch_search()
        {
            return Err(EffectError::WrongLibraryTopFrame);
        }
        if actor != bound.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let unique = selected.iter().copied().collect::<BTreeSet<_>>();
        if (bound.requires_search_result()
            && selected.len()
                != bound
                    .search_batch_limit()
                    .min(bound.eligible_results().len()))
            || selected.len() > bound.search_batch_limit()
            || unique.len() != selected.len()
            || selected
                .iter()
                .any(|id| !bound.eligible_results().contains(id))
        {
            return Err(EffectError::InvalidLibraryPartition);
        }
        let destination = if bound.is_exile_batch_search() {
            ObjectDestination::Exile
        } else if matches!(
            self.current_instruction()?,
            Instruction::SearchLibrary { filter }
                if filter.destination == SearchDestination::Graveyard
        ) {
            ObjectDestination::Graveyard
        } else {
            ObjectDestination::Hand
        };
        let moves = selected.into_iter().map(|id| (id, destination)).collect();
        self.commit_search_batch(revision, bound, actor, moves, (false, &[], &[]), entropy)
    }
    /// The controller supplies the relative timestamp order for simultaneous entries.
    // Public-boundary battlefield-search resume signature kept flat for direct caller ergonomics.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resume_battlefield_search(
        &self,
        expected: u64,
        bound: &LibraryTopFrame,
        actor: SeatId,
        selected: Vec<ObjectId>,
        colors: &[(ObjectId, crate::mana::ManaType)],
        reveals: &[(ObjectId, Option<ObjectId>)],
        entropy: &EntropyTape,
    ) -> Result<(RuleProgress, Option<(EntropyTape, ShuffleRecord)>), EffectError> {
        let revision = self.next_revision(expected)?;
        if bound != &self.search_frame(self.current_instruction()?)?
            || !bound.is_battlefield_batch_search()
        {
            return Err(EffectError::WrongLibraryTopFrame);
        }
        if actor != bound.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let Instruction::SearchLibrary { filter } = self.current_instruction()? else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        let max_selected = match &filter.count {
            SearchCount::UpTo(n) => usize::from(*n),
            SearchCount::UpToExpr(expr) => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongLibraryTopFrame)?;
                self.resolve_value_expr(entry.controller(), expr)?
                    .max(0.into())
                    .to_usize()
                    .unwrap_or(0)
            }
            SearchCount::Exact(n) => usize::from(n.get()),
            SearchCount::UpToPlayersWithMoreLands => 2,
        };
        let unique: BTreeSet<_> = selected.iter().copied().collect();
        if selected.len() > max_selected
            || unique.len() != selected.len()
            || selected
                .iter()
                .any(|id| !bound.eligible_results().contains(id))
        {
            return Err(EffectError::InvalidLibraryPartition);
        }
        if selected.len() == 2
            && let Some(constraint) = filter.pair_constraint
        {
            let evaluated = self.evaluated_characteristics()?;
            let left = &evaluated
                .get(selected[0])
                .ok_or(EffectError::InconsistentProgram)?
                .current_land_subtypes;
            let right = &evaluated
                .get(selected[1])
                .ok_or(EffectError::InconsistentProgram)?
                .current_land_subtypes;
            let valid = match constraint {
                SearchPairConstraint::ShareLandType => !left.is_disjoint(right),
                SearchPairConstraint::DistinctLandTypes(first, second) => {
                    (left.contains(&first) && right.contains(&second))
                        || (left.contains(&second) && right.contains(&first))
                }
            };
            if !valid {
                return Err(EffectError::InvalidLibraryPartition);
            }
        }
        let tapped = filter.enters_tapped();
        let moves = selected
            .into_iter()
            .map(|id| (id, ObjectDestination::Battlefield { controller: actor }))
            .collect();
        self.commit_search_batch(
            revision,
            bound,
            actor,
            moves,
            (tapped, colors, reveals),
            entropy,
        )
    }

    // Tuple bundles tapped state with optional entry-color and entry-reveal slices.
    #[allow(clippy::type_complexity)]
    fn commit_search_batch(
        &self,
        revision: u64,
        bound: &LibraryTopFrame,
        actor: SeatId,
        moves: Vec<(ObjectId, ObjectDestination)>,
        entry: (
            bool,
            &[(ObjectId, crate::mana::ManaType)],
            &[(ObjectId, Option<ObjectId>)],
        ),
        entropy: &EntropyTape,
    ) -> Result<(RuleProgress, Option<(EntropyTape, ShuffleRecord)>), EffectError> {
        let (tapped, colors, reveals) = entry;
        let entering: Vec<_> = moves
            .iter()
            .filter_map(|(id, destination)| {
                matches!(destination, ObjectDestination::Battlefield { .. }).then_some(*id)
            })
            .collect();
        self.search_entry_colors(&entering, colors)?;
        self.search_entry_reveals(bound, &entering, reveals)?;
        let graveyard_search = bound.is_hand_graveyard_search()
            || matches!(
                self.current_instruction()?,
                Instruction::SearchLibrary { filter }
                    if filter.destination == SearchDestination::Graveyard
            );
        let exile_search = matches!(
            self.current_instruction()?,
            Instruction::SearchLibrary { filter } if filter.is_exile_destination()
        );
        let affected: Vec<_> = if exile_search {
            moves.iter().map(|(id, _)| *id).collect()
        } else if graveyard_search {
            moves
                .iter()
                .filter_map(|(id, destination)| {
                    (*destination == ObjectDestination::Graveyard).then_some(*id)
                })
                .collect()
        } else {
            entering.clone()
        };
        if !affected.is_empty() {
            // A later replacement or entry choice must never strand an already
            // accepted search because the host supplied too little entropy.
            // A selected Aura can remain in the library without a legal host.
            // Entry-copy replacements can create the same outcome after selection.
            let remaining = bound
                .domain_top_first()
                .iter()
                .copied()
                .filter(|id| !moves.iter().any(|(selected, _)| selected == id))
                .collect::<Vec<_>>();
            let evaluated = self.evaluated_characteristics()?;
            let restrict_entry = Self::graveyard_and_library_restricted(&evaluated);
            let possible_stays = entering
                .iter()
                .filter(|id| {
                    bound.domain_top_first().contains(id)
                        && (self.copiable_program(**id).is_some_and(|program| {
                            program.entry_copy.is_some()
                                || matches!(
                                    program.permanent().and_then(|p| p.attachment),
                                    Some(crate::attachment::AttachmentKind::Aura(_))
                                )
                        }) || restrict_entry
                            && evaluated
                                .get(**id)
                                .is_some_and(ObjectCharacteristics::is_creature))
                })
                .copied()
                .collect::<Vec<_>>();
            for length in 0..=possible_stays.len() {
                let possible = remaining
                    .iter()
                    .copied()
                    .chain(possible_stays.iter().take(length).copied())
                    .collect();
                entropy.shuffled(possible).map_err(|error| {
                    EffectError::Window(StackWindowError::Objects(
                        crate::object_identity::ObjectIdentityError::Entropy(error),
                    ))
                })?;
            }
        }
        let mut next = self.clone();
        next.revision = revision;
        if !affected.is_empty() {
            let cursor = next
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?
                .clone();
            let reason = if exile_search {
                ZoneReason::ExiledByEffect
            } else if graveyard_search {
                ZoneReason::SearchToGraveyard
            } else if matches!(
                next.current_instruction()?,
                Instruction::SearchLibrary { filter } if filter.is_transfigure()
            ) {
                ZoneReason::TransfigureEntry
            } else {
                ZoneReason::SearchToBattlefield
            };
            let action = if exile_search {
                crate::program::ZoneActionKind::EffectExile
            } else if graveyard_search {
                crate::program::ZoneActionKind::EffectSearchToGraveyard
            } else {
                crate::program::ZoneActionKind::EffectEnterBattlefield
            };
            let input = zone_replacement::AuthorizedZoneBatch {
                cause: EventCause::Instruction {
                    entry: cursor.entry,
                    instruction: cursor.instruction,
                },
                action,
                intents: affected
                    .iter()
                    .map(|id| zone_replacement::ZoneMoveIntent {
                        before: *id,
                        reason,
                    })
                    .collect(),
            };
            let batch = if graveyard_search || exile_search {
                next.begin_zone_replacements(input)?
            } else {
                next.begin_zone_replacements_with_entry(
                    input,
                    Some(&affected.iter().map(|id| (*id, actor)).collect()),
                )?
            };
            if !matches!(next.pending_work, Some(PendingWork::Resolution(_))) {
                return Err(EffectError::WrongResolutionFrame);
            }
            let fixed_aura_host = match next.current_instruction()? {
                Instruction::SearchLibrary { filter } => match filter.curse_host() {
                    Some(host) => next.searched_curse_host(host)?,
                    None => None,
                },
                _ => None,
            };
            next.pending_work = Some(PendingWork::SearchEntry(Box::new(SearchEntryWork {
                cursor,
                bound: bound.clone(),
                actor,
                fixed_aura_host,
                moves,
                tapped,
                colors: colors.to_vec(),
                reveals: reveals.to_vec(),
                chapters: Vec::new(),
                creature_types: Vec::new(),
                entropy: entropy.clone(),
                batch,
                reason,
                action,
                graveyard_order: None,
            })));
            let progress = next.progress_rules(
                DamageJournal::default(),
                vec![],
                vec![],
                DrawJournal::default(),
                RulesJournal::default(),
            )?;
            return Ok((progress, None));
        }
        let mut journal = RulesJournal::default();
        let (entropy, shuffle) = next.commit_search_moves(
            bound,
            actor,
            &moves,
            &[],
            ZoneReason::SearchToBattlefield,
            tapped,
            colors,
            reveals,
            &[],
            &[],
            entropy,
            &mut journal,
        )?;
        let progress = next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            journal,
        )?;
        Ok((progress, Some((entropy, shuffle))))
    }

    fn search_aura_domain(
        &self,
        work: &SearchEntryWork,
        id: ObjectId,
        current: &EvaluatedCharacteristics,
    ) -> Option<Vec<TargetRef>> {
        if work.batch.destination(id).ok()? != crate::program::PublicZoneDestination::Battlefield {
            return None;
        }
        let controller = work.batch.entry_controller(id).unwrap_or(work.actor);
        self.prospective_aura_domain(
            id,
            controller,
            work.batch.copied_definition(id),
            work.fixed_aura_host,
            current,
        )
    }

    pub(super) fn search_aura_frame(
        &self,
        work: &SearchEntryWork,
    ) -> Result<Option<LinkedAuraChoiceFrame>, EffectError> {
        if work.fixed_aura_host.is_some() {
            return Ok(None);
        }
        for &(id, destination) in &work.moves {
            if !matches!(destination, ObjectDestination::Battlefield { .. })
                || self.resolution_aura_choices.contains_key(&id)
                || !matches!(
                    work.batch
                        .copied_definition(id)
                        .or_else(|| self.copiable_program(id))
                        .and_then(Program::permanent)
                        .and_then(|p| p.attachment),
                    Some(crate::attachment::AttachmentKind::Aura(_))
                )
            {
                continue;
            }
            if let Some(domain) = self.search_aura_domain(work, id, work.batch.evaluated())
                && !domain.is_empty()
            {
                return Ok(Some(LinkedAuraChoiceFrame {
                    revision: self.revision,
                    chooser: work.batch.entry_controller(id).unwrap_or(work.actor),
                    object: id,
                    domain,
                }));
            }
        }
        Ok(None)
    }

    pub(super) fn search_entry_choice_frame(
        &self,
        work: &SearchEntryWork,
    ) -> Result<Option<EntryChoiceFrame>, EffectError> {
        for &(object, destination) in &work.moves {
            if !matches!(destination, ObjectDestination::Battlefield { .. })
                || work.batch.destination(object)?
                    != crate::program::PublicZoneDestination::Battlefield
            {
                continue;
            }
            let chooser = work.batch.entry_controller(object).unwrap_or(work.actor);
            if self
                .search_aura_domain(work, object, work.batch.evaluated())
                .is_some_and(|domain| domain.is_empty())
            {
                continue;
            }
            let Some(definition) = work
                .batch
                .copied_definition(object)
                .or_else(|| self.copiable_program(object))
            else {
                continue;
            };
            let Some(rule) = definition
                .permanent()
                .and_then(|permanent| permanent.entry_tap)
            else {
                continue;
            };
            let chosen = match rule {
                crate::land::EntryTap::AlwaysChooseColor
                | crate::land::EntryTap::AlwaysChooseColorExcept(_)
                | crate::land::EntryTap::ChooseColor
                | crate::land::EntryTap::ChooseColorExcept(_) => {
                    work.colors.iter().any(|(id, _)| *id == object)
                }
                crate::land::EntryTap::ReadAhead { .. } => {
                    work.chapters.iter().any(|(id, _)| *id == object)
                }
                crate::land::EntryTap::ChooseCreatureType => {
                    work.creature_types.iter().any(|(id, _)| *id == object)
                }
                crate::land::EntryTap::UnlessRevealHandLandType(_, _)
                | crate::land::EntryTap::UnlessRevealHandSubtype { .. } => {
                    work.reveals.iter().any(|(id, _)| *id == object)
                }
                _ => self.resolution_entry_choices.contains_key(&object),
            };
            if !chosen
                && let Some(frame) =
                    self.entry_choice_for_definition(chooser, object, definition, false)?
            {
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }

    pub(super) fn progress_search_entry(
        &mut self,
        journal: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::SearchEntry(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::SearchEntry(work));
            return Ok(Some(frame));
        }
        if let Some(frame) = work.graveyard_frame(self.revision, &self.window)? {
            self.pending_work = Some(PendingWork::SearchEntry(work));
            return Ok(Some(EffectDecision::LinkedReturnOrder(frame)));
        }
        if let Some(frame) = self.search_aura_frame(&work)? {
            self.pending_work = Some(PendingWork::SearchEntry(work));
            return Ok(Some(EffectDecision::LinkedAuraChoice(frame)));
        }
        if let Some(frame) = self.search_entry_choice_frame(&work)? {
            self.pending_work = Some(PendingWork::SearchEntry(work));
            return Ok(Some(EffectDecision::EntryChoice(frame)));
        }
        let entering = work
            .moves
            .iter()
            .filter_map(|(id, d)| {
                (matches!(d, ObjectDestination::Battlefield { .. })
                    || work.action == crate::program::ZoneActionKind::EffectExile
                        && *d == ObjectDestination::Exile
                    || work.action == crate::program::ZoneActionKind::EffectSearchToGraveyard
                        && *d == ObjectDestination::Graveyard)
                    .then_some(*id)
            })
            .collect::<Vec<_>>();
        let reason = work.reason;
        let input = zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
            },
            action: work.action,
            intents: entering
                .iter()
                .map(|id| zone_replacement::ZoneMoveIntent {
                    before: *id,
                    reason,
                })
                .collect(),
        };
        let fresh = if work.action == crate::program::ZoneActionKind::EffectEnterBattlefield {
            self.begin_zone_replacements_with_entry(
                input,
                Some(&entering.iter().map(|id| (*id, work.actor)).collect()),
            )?
        } else {
            self.begin_zone_replacements(input)?
        };
        work.batch.verify_initial(fresh)?;
        let mut cannot_enter: BTreeSet<_> = entering
            .iter()
            .copied()
            .filter(|&id| work.batch.entry_prohibited(id))
            .collect();
        for &(id, destination) in &work.moves {
            if matches!(destination, ObjectDestination::Battlefield { .. })
                && let Some(domain) = self.search_aura_domain(&work, id, work.batch.evaluated())
            {
                if domain.is_empty() {
                    cannot_enter.insert(id);
                } else if let Some(host) = work.fixed_aura_host {
                    self.resolution_aura_choices.insert(id, host);
                } else if self
                    .resolution_aura_choices
                    .get(&id)
                    .is_none_or(|host| !domain.contains(host))
                {
                    return Err(EffectError::IllegalTarget);
                }
            }
        }
        work.moves.retain(|(id, _)| !cannot_enter.contains(id));
        work.batch.commit_devour(self, journal)?;
        self.prospective_entry_copies.extend(
            work.batch
                .entry_copies()
                .filter(|(id, _)| !cannot_enter.contains(id)),
        );
        self.prospective_entry_controllers.extend(
            work.batch
                .entry_controllers()
                .filter(|(id, _)| !cannot_enter.contains(id)),
        );
        for (id, dest) in &mut work.moves {
            if entering.contains(id) {
                *dest = work
                    .batch
                    .destination(*id)?
                    .physical(Some(work.actor))
                    .ok_or(EffectError::InconsistentProgram)?;
            }
        }
        if let Some(order) = &work.graveyard_order {
            work.moves.sort_by_key(|(id, _)| {
                order
                    .iter()
                    .rev()
                    .position(|x| x == id)
                    .unwrap_or(order.len())
            });
        }
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        let (entropy, shuffle) = self.commit_search_moves(
            &work.bound,
            work.actor,
            &work.moves,
            &entering,
            work.reason,
            work.tapped,
            &work.colors,
            &work.reveals,
            &work.chapters,
            &work.creature_types,
            &work.entropy,
            journal,
        )?;
        self.completed_search_shuffle = Some((entropy, shuffle));
        Ok(None)
    }

    // Internal search-commit helper forwarding resolved entry parameters atomically.
    #[allow(clippy::too_many_arguments)]
    fn commit_search_moves(
        &mut self,
        bound: &LibraryTopFrame,
        actor: SeatId,
        moves: &[(ObjectId, ObjectDestination)],
        entering: &[ObjectId],
        entry_reason: ZoneReason,
        tapped: bool,
        colors: &[(ObjectId, crate::mana::ManaType)],
        reveals: &[(ObjectId, Option<ObjectId>)],
        chapters: &[(ObjectId, u8)],
        creature_types: &[(
            ObjectId,
            spellwake_contracts::creature_types::CreatureTypeId,
        )],
        entropy: &EntropyTape,
        journal: &mut RulesJournal,
    ) -> Result<(EntropyTape, ShuffleRecord), EffectError> {
        let colors = colors.iter().copied().collect::<BTreeMap<_, _>>();
        let reveals = reveals.iter().copied().collect::<BTreeMap<_, _>>();
        let creature_types = creature_types.iter().copied().collect::<BTreeMap<_, _>>();
        let haste_and_return = matches!(
            self.current_instruction()?,
            Instruction::SearchLibrary { filter } if filter.is_haste_and_return()
        );
        let mut found = None;
        let mut moved_ids: Vec<ObjectId> = Vec::new();
        if !moves.is_empty() {
            let before = self.rules_snapshot(None)?;
            let from_graveyard = matches!(
                self.current_instruction()?,
                Instruction::SearchLibrary { filter }
                    if filter.source == SearchSource::OwnLibraryOrGraveyard
            ) && matches!(moves, [(id, _)] if self.window.cards().get(*id).is_some_and(|card| card.zone() == ObjectZone::Graveyard(actor)) && bound.eligible_results().contains(id));
            let (window, mut movements) = if from_graveyard {
                let actual: Vec<_> = moves
                    .iter()
                    .copied()
                    .filter(|(_, dest)| *dest != ObjectDestination::Graveyard)
                    .collect();
                if actual.is_empty() {
                    (self.window.clone(), vec![])
                } else {
                    self.window.record_linked_return_moves(&actual)?
                }
            } else {
                self.window.record_library_search_moves(
                    self.window.revision(),
                    bound.entry(),
                    bound.search_exile_owner().unwrap_or(actor),
                    actor,
                    moves,
                )?
            };
            self.window = window;
            moved_ids = movements.iter().map(|m| m.after.id()).collect();
            if matches!(
                self.current_instruction()?,
                Instruction::SearchLibrary { filter }
                    if filter.destination == SearchDestination::Exile { link: SearchExileLink::FreeCastCohort }
            ) {
                let cursor = self
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                cursor.found_spells.extend(
                    movements
                        .iter()
                        .filter(|m| m.after.zone() == ObjectZone::Exile)
                        .map(|m| m.after.id()),
                );
            } else if entry_reason == ZoneReason::ExiledByEffect {
                let source = self
                    .execution_bindings(bound.entry())
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?
                    .source
                    .object
                    .id();
                for movement in &movements {
                    if movement.after.zone() == ObjectZone::Exile {
                        self.linked_search_results
                            .entry(source)
                            .or_default()
                            .push(movement.after.id());
                    }
                }
            }
            let entrants = movements
                .iter()
                .filter(|movement| movement.after.zone() == ObjectZone::Battlefield)
                .map(|movement| movement.after.id())
                .collect::<Vec<_>>();
            self.characteristics
                .register_battlefield_entries(&entrants)?;
            for movement in &movements {
                if movement.after.zone() == ObjectZone::Battlefield {
                    let id = movement.after.id();
                    self.stage_resolution_entry_choice(movement.before.id(), id)?;
                    if haste_and_return {
                        found = Some(id);
                    }
                    if let Some(color) = colors.get(&movement.before.id()) {
                        self.entry_color_choices.insert(id, *color);
                        if let Some(c) = color.color() {
                            self.characteristics.entry_color_choices.insert(id, c);
                        }
                    }
                    if let Some((_, number)) = chapters
                        .iter()
                        .find(|(old, _)| *old == movement.before.id())
                    {
                        self.entry_choices
                            .insert(id, crate::land::EntryChoice::Chapter(*number));
                    }
                    if let Some(kind) = creature_types.get(&movement.before.id()) {
                        self.entry_creature_type_choices.insert(id, *kind);
                    }
                    if let Some(reveal) = reveals.get(&movement.before.id()) {
                        self.entry_choices
                            .insert(id, crate::land::EntryChoice::Reveal(*reveal));
                    }
                    if let Some(host) = self.resolution_aura_choices.remove(&movement.before.id()) {
                        self.characteristics.attach(id, host, true)?;
                    }
                    if tapped {
                        self.window.enter_tapped(id)?;
                    }
                }
            }
            let mut counters = Vec::new();
            for movement in &mut movements {
                if movement.after.zone() == ObjectZone::Battlefield {
                    let id = movement.after.id();
                    let added = self.apply_entry_counters(
                        &before,
                        id,
                        PowerToughnessCounters::default(),
                        0,
                    )?;
                    movement.after = self
                        .window
                        .cards()
                        .get(id)
                        .ok_or(EffectError::InconsistentProgram)?
                        .clone();
                    if added.valid_placement() {
                        counters.push((movement.before.id(), id, added));
                    }
                }
            }
            self.emit_from(
                EventCause::Instruction {
                    entry: bound.entry(),
                    instruction: bound.instruction(),
                },
                &before,
                None,
                journal,
                |previous, current, controllers| {
                    let mut rows = Vec::new();
                    for movement in &movements {
                        let reason = if entering.contains(&movement.before.id()) {
                            entry_reason
                        } else {
                            ZoneReason::SearchToHand
                        };
                        rows.push(zone_occurrence_at(
                            previous,
                            current,
                            movement,
                            reason,
                            controllers,
                        )?);
                    }
                    for (old, id, added) in &counters {
                        rows.push(RulesOccurrence::CountersPlaced {
                            object: *id,
                            before: previous.characteristics.counters(*id),
                            added: added.clone(),
                            after: current.characteristics.counters(*id),
                            before_appearance: Box::new(
                                previous
                                    .appearance(*old)
                                    .ok_or(EffectError::InconsistentProgram)?,
                            ),
                            after_appearance: Box::new(
                                current
                                    .appearance(*id)
                                    .ok_or(EffectError::InconsistentProgram)?,
                            ),
                        });
                    }
                    Ok(rows)
                },
            )?;
        }
        let shuffle_before = self.rules_snapshot(None)?;
        let (window, entropy, shuffle) = self
            .window
            .search_shuffle(bound.search_exile_owner().unwrap_or(actor), entropy)?;
        self.window = window;
        self.emit_from(
            EventCause::Instruction {
                entry: bound.entry(),
                instruction: bound.instruction(),
            },
            &shuffle_before,
            None,
            journal,
            |_, _, _| Ok(vec![RulesOccurrence::Shuffled(shuffle.clone())]),
        )?;
        if let Some(object) = found {
            use crate::characteristics::{
                AbilityChange, ResolvedDuration, ResolvedPart, ResolvedParts,
            };
            let before = self.rules_snapshot(None)?;
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let StackKind::ActivatedAbility { source } = entry.kind() else {
                return Err(EffectError::InconsistentProgram);
            };
            let appearance = before
                .view()
                .appearance(source.id())
                .or_else(|| self.departed_reflexive_sources.get(&source.id()).cloned())
                .ok_or(EffectError::InconsistentProgram)?;
            self.triggers
                .delay_permanent_return_to_hand(appearance, entry.controller(), object);
            let effect = self.characteristics.add_effect(
                entry.id(),
                object,
                ResolvedParts(vec![ResolvedPart::Abilities(
                    AbilityChange::grant_keywords([KeywordAbility::Haste]),
                )]),
                ResolvedDuration::NoStatedDuration,
            )?;
            self.emit_from(
                EventCause::Instruction {
                    entry: bound.entry(),
                    instruction: bound.instruction(),
                },
                &before,
                None,
                journal,
                |old, new, _| {
                    let appearances = occurrence_flow::characteristic_appearance_changes(
                        old,
                        new,
                        effect.affected.iter().copied(),
                    )?;
                    Ok(vec![RulesOccurrence::ContinuousEffectCreated {
                        effect,
                        appearances,
                    }])
                },
            )?;
        }
        if let Some(cursor) = self.resolution_cursor_mut() {
            cursor.results.last_action_count =
                Some(i64::try_from(moved_ids.len()).unwrap_or(i64::MAX).into());
            cursor.results.last_action_succeeded = !moved_ids.is_empty();
            cursor.results.last_moved_objects = moved_ids;
            cursor.results.last_action_lki.clear();
        }
        self.advance_instruction()?;
        Ok((entropy, shuffle))
    }

    fn search_entry_colors(
        &self,
        entering: &[ObjectId],
        colors: &[(ObjectId, crate::mana::ManaType)],
    ) -> Result<BTreeMap<ObjectId, crate::mana::ManaType>, EffectError> {
        let mut provided = BTreeMap::new();
        for &(object, color) in colors {
            if !entering.contains(&object) || provided.insert(object, color).is_some() {
                return Err(EffectError::IllegalEntryColor);
            }
            let rule = self
                .copiable_program(object)
                .and_then(Program::permanent)
                .and_then(|value| value.entry_tap);
            let color_ok = match rule {
                Some(
                    crate::land::EntryTap::AlwaysChooseColorExcept(excluded)
                    | crate::land::EntryTap::ChooseColorExcept(excluded),
                ) => color != crate::mana::ManaType::Colourless && color != excluded,
                Some(
                    crate::land::EntryTap::ChooseColor | crate::land::EntryTap::AlwaysChooseColor,
                ) => color != crate::mana::ManaType::Colourless,
                _ => false,
            };
            if !color_ok {
                return Err(EffectError::IllegalEntryColor);
            }
        }
        Ok(provided)
    }

    fn search_entry_reveals(
        &self,
        bound: &LibraryTopFrame,
        entering: &[ObjectId],
        reveals: &[(ObjectId, Option<ObjectId>)],
    ) -> Result<BTreeMap<ObjectId, Option<ObjectId>>, EffectError> {
        let mut provided = BTreeMap::new();
        for &(object, reveal) in reveals {
            let Some(eligible) = bound.search_entry_reveal_options().get(&object) else {
                return Err(EffectError::IllegalTarget);
            };
            if !entering.contains(&object)
                || provided.insert(object, reveal).is_some()
                || reveal.is_some_and(|card| !eligible.contains(&card))
            {
                return Err(EffectError::IllegalTarget);
            }
        }
        Ok(provided)
    }
}
