//! Continuous visibility and play permissions for the controller's actual library top.
use super::*;
use crate::characteristics::LibraryTopPermission;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LibraryTopAccess {
    pub visible: bool,
    pub playable: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct StaticExileCastGrant {
    pub(super) allowed: bool,
    pub(super) free: bool,
    pub(super) any_color: bool,
    pub(super) consumed_once: Option<(ObjectId, crate::characteristics::StaticAbilityId)>,
}

impl EffectWindow {
    fn filter_allows_land_type(filter: crate::program::ObjectFilter) -> bool {
        let clause_allows_land = |c: crate::program::ObjectClause| {
            !c.forbidden_types.contains(CardType::Land)
                && (c.any_types.is_empty() || c.any_types.contains(CardType::Land))
                && (c.required_types.is_empty()
                    || c.required_types == crate::program::CardTypeMask::LAND)
        };
        match filter {
            crate::program::ObjectFilter::Clause(c) => clause_allows_land(c),
            crate::program::ObjectFilter::Either(a, b) => {
                clause_allows_land(a) || clause_allows_land(b)
            }
        }
    }

    fn matches_zone_play_filter(
        &self,
        lands_allowed: bool,
        filter: crate::program::ObjectFilter,
        card: &crate::characteristics::ObjectCharacteristics,
        controller: SeatId,
        source: Option<ObjectId>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        let excluded = BTreeSet::new();
        if card.has_type(CardType::Land) {
            lands_allowed
                && (!Self::filter_allows_land_type(filter)
                    || self.matches_object_filter(
                        filter, card, controller, source, &excluded, evaluated,
                    ))
        } else {
            self.matches_object_filter(filter, card, controller, source, &excluded, evaluated)
        }
    }

    fn matches_library_top_permission(
        matching: LibraryTopPermission,
        card: &crate::characteristics::ObjectCharacteristics,
        entry_creature_type: Option<spellwake_contracts::creature_types::CreatureTypeId>,
    ) -> bool {
        match matching {
            LibraryTopPermission::All => true,
            LibraryTopPermission::Land => card.has_type(CardType::Land),
            LibraryTopPermission::Creature => card.is_creature(),
            LibraryTopPermission::Artifact => card.has_type(CardType::Artifact),
            LibraryTopPermission::Nonland => !card.has_type(CardType::Land),
            LibraryTopPermission::InstantOrSorcery => {
                card.has_type(CardType::Instant) || card.has_type(CardType::Sorcery)
            }
            LibraryTopPermission::ArtifactOrColorlessSpell => {
                !card.has_type(CardType::Land)
                    && (card.has_type(CardType::Artifact) || card.current_colors.is_empty())
            }
            LibraryTopPermission::CreatureOfSourceEntryType => {
                card.is_creature()
                    && entry_creature_type
                        .is_some_and(|kind| card.current_creature_subtypes.contains(&kind))
            }
        }
    }

    pub(super) fn is_linked_exile_of(&self, source: ObjectId, card: ObjectId) -> bool {
        self.characteristics
            .linked_exiles
            .get(&source)
            .is_some_and(|list| list.contains(&card))
            || self
                .independent_exile_links
                .iter()
                .any(|((src, _), list)| *src == source && list.contains(&card))
            || self
                .characteristics
                .imprinted_cards
                .get(&source)
                .is_some_and(|list| list.contains(&card))
    }

    /// Evaluate live library-top permissions against the current characteristic cut.
    /// Only the exact current top card can be disclosed or played.
    pub fn library_top_access(
        &self,
        evaluated: &EvaluatedCharacteristics,
        owner: SeatId,
        viewer: SeatId,
    ) -> LibraryTopAccess {
        let mut access = LibraryTopAccess::default();
        let Some(&id) = self.window.cards().library(owner).first() else {
            return access;
        };
        // CR 401.5. A changed top stays hidden during a suspended announcement.
        let frozen = self
            .deferred_cast
            .as_ref()
            .map(|cast| &cast.library_tops)
            .or_else(|| {
                self.deferred_mana_cast
                    .as_ref()
                    .map(|cast| &cast.prepared.library_tops)
            });
        if frozen.is_some_and(|before| before.get(&owner) != Some(&id)) {
            return access;
        }
        let Some(card) = evaluated.get(id) else {
            return access;
        };
        access.visible = self.currently_revealed_look_top(id);
        if let Some(PendingWork::OptionalEffect(work)) = &self.pending_work
            && work.cast.as_ref().is_some_and(|offer| {
                offer.ability == ExileCastAbility::RevealedLibrary && offer.spell == id
            })
        {
            access.visible = true;
        }
        let apply_ability = |access: &mut LibraryTopAccess,
                             ability: &crate::characteristics::StaticContinuousAbility,
                             ctrl: Option<SeatId>,
                             source_id: Option<ObjectId>,
                             entry_type: Option<
            spellwake_contracts::creature_types::CreatureTypeId,
        >| {
            match ability {
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::AllPlayersRevealLibraryTop,
                    ..
                } => access.visible = true,
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission: crate::characteristics::CastPermissionRule::SeeLibraryTop { public },
                    ..
                } if ctrl == Some(owner) => {
                    access.visible |= *public || viewer == owner;
                }
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission: crate::characteristics::CastPermissionRule::PlayLibraryTop(matching),
                    ..
                } if ctrl == Some(owner) && viewer == owner => {
                    access.playable |=
                        Self::matches_library_top_permission(*matching, card, entry_type);
                }
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::PlayLibraryTopMatching {
                            lands_allowed,
                            filter,
                        },
                    ..
                } if ctrl == Some(owner) && viewer == owner => {
                    access.playable |= self.matches_zone_play_filter(
                        *lands_allowed,
                        **filter,
                        card,
                        owner,
                        source_id,
                        evaluated,
                    );
                }
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    id: ability_id,
                    permission:
                        crate::characteristics::CastPermissionRule::PlayLibraryTopFiltered {
                            lands_allowed,
                            filter,
                            once_each_turn,
                            own_turn_only,
                        },
                } if ctrl == Some(owner)
                    && viewer == owner
                    && (!*own_turn_only || self.window.active_player() == owner) =>
                {
                    access.visible = true;
                    let used = *once_each_turn
                        && source_id
                            .is_some_and(|src| self.source_cast_uses.contains(&(src, *ability_id)));
                    if !used {
                        access.playable |= self.matches_zone_play_filter(
                            *lands_allowed,
                            **filter,
                            card,
                            owner,
                            source_id,
                            evaluated,
                        );
                    }
                }
                _ => {}
            }
        };
        for source in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            for ability in source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
            {
                if source.abilities.continuous != Some(ability.id())
                    && !source
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ability.id())
                {
                    continue;
                }
                apply_ability(
                    &mut access,
                    ability,
                    source.controller,
                    Some(source.object),
                    source.entry_creature_type,
                );
            }
        }
        for (seat, ability) in &evaluated.emblem_rules {
            apply_ability(&mut access, ability, Some(*seat), None, None);
        }
        for active in &self.temporary_rules {
            if let Some(ability) = self.active_temporary_static(active, evaluated) {
                apply_ability(
                    &mut access,
                    ability,
                    Some(active.controller),
                    active.source_object,
                    None,
                );
            }
        }
        access
    }

    pub(super) fn library_top_once_permission(
        &self,
        evaluated: &EvaluatedCharacteristics,
        id: ObjectId,
        actor: SeatId,
    ) -> Option<(ObjectId, crate::characteristics::StaticAbilityId)> {
        if self.window.cards().library(actor).first() != Some(&id) {
            return None;
        }
        let card = evaluated.get(id)?;
        let mut once_candidate = None;
        for source in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield && p.controller == Some(actor))
        {
            for ability in source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
            {
                if source.abilities.continuous != Some(ability.id())
                    && !source
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ability.id())
                {
                    continue;
                }
                match ability {
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission:
                            crate::characteristics::CastPermissionRule::PlayLibraryTop(matching),
                        ..
                    } if Self::matches_library_top_permission(
                        *matching,
                        card,
                        source.entry_creature_type,
                    ) =>
                    {
                        return None;
                    }
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission:
                            crate::characteristics::CastPermissionRule::PlayLibraryTopMatching {
                                lands_allowed,
                                filter,
                            },
                        ..
                    } if self.matches_zone_play_filter(
                        *lands_allowed,
                        **filter,
                        card,
                        actor,
                        Some(source.object),
                        evaluated,
                    ) =>
                    {
                        return None;
                    }
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        id: ability_id,
                        permission:
                            crate::characteristics::CastPermissionRule::PlayLibraryTopFiltered {
                                lands_allowed,
                                filter,
                                once_each_turn,
                                own_turn_only,
                            },
                    } if (!*own_turn_only || self.window.active_player() == actor)
                        && self.matches_zone_play_filter(
                            *lands_allowed,
                            **filter,
                            card,
                            actor,
                            Some(source.object),
                            evaluated,
                        ) =>
                    {
                        if !*once_each_turn {
                            return None;
                        }
                        if once_candidate.is_none()
                            && !self
                                .source_cast_uses
                                .contains(&(source.object, *ability_id))
                        {
                            once_candidate = Some((source.object, *ability_id));
                        }
                    }
                    _ => {}
                }
            }
        }
        for (seat, ability) in &evaluated.emblem_rules {
            if *seat != actor {
                continue;
            }
            match ability {
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission: crate::characteristics::CastPermissionRule::PlayLibraryTop(matching),
                    ..
                } if Self::matches_library_top_permission(*matching, card, None) => {
                    return None;
                }
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::PlayLibraryTopMatching {
                            lands_allowed,
                            filter,
                        },
                    ..
                } if self.matches_zone_play_filter(
                    *lands_allowed,
                    **filter,
                    card,
                    actor,
                    None,
                    evaluated,
                ) =>
                {
                    return None;
                }
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::PlayLibraryTopFiltered {
                            lands_allowed,
                            filter,
                            once_each_turn: false,
                            own_turn_only,
                        },
                    ..
                } if (!*own_turn_only || self.window.active_player() == actor)
                    && self.matches_zone_play_filter(
                        *lands_allowed,
                        **filter,
                        card,
                        actor,
                        None,
                        evaluated,
                    ) =>
                {
                    return None;
                }
                _ => {}
            }
        }
        for active in &self.temporary_rules {
            if active.controller != actor {
                continue;
            }
            let Some(ability) = self.active_temporary_static(active, evaluated) else {
                continue;
            };
            match ability {
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission: crate::characteristics::CastPermissionRule::PlayLibraryTop(matching),
                    ..
                } if Self::matches_library_top_permission(*matching, card, None) => {
                    return None;
                }
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::PlayLibraryTopMatching {
                            lands_allowed,
                            filter,
                        },
                    ..
                } if self.matches_zone_play_filter(
                    *lands_allowed,
                    **filter,
                    card,
                    actor,
                    active.source_object,
                    evaluated,
                ) =>
                {
                    return None;
                }
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::PlayLibraryTopFiltered {
                            lands_allowed,
                            filter,
                            once_each_turn: false,
                            own_turn_only,
                        },
                    ..
                } if (!*own_turn_only || self.window.active_player() == actor)
                    && self.matches_zone_play_filter(
                        *lands_allowed,
                        **filter,
                        card,
                        actor,
                        active.source_object,
                        evaluated,
                    ) =>
                {
                    return None;
                }
                _ => {}
            }
        }
        once_candidate
    }

    /// True if `owner`'s hand is continuously revealed to `viewer` under an active static effect
    /// (for example Telepathy, Revelation, Wandering Eye).
    pub fn hand_revealed_to(
        &self,
        evaluated: &EvaluatedCharacteristics,
        owner: SeatId,
        viewer: SeatId,
    ) -> bool {
        if owner == viewer {
            return true;
        }
        evaluated.objects.values().any(|source| {
            source.zone == ObjectZone::Battlefield
                && source.has_battlefield_rule(|rule| match rule {
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission:
                            crate::characteristics::CastPermissionRule::RevealHands {
                                opponents_only,
                            },
                        ..
                    } => !opponents_only || source.controller.is_some_and(|ctrl| ctrl != owner),
                    _ => false,
                })
        })
    }

    pub(super) fn library_play_permitted(
        &self,
        id: ObjectId,
        actor: SeatId,
    ) -> Result<bool, EffectError> {
        if self.window.cards().library(actor).first() != Some(&id) {
            return Ok(false);
        }
        Ok(self
            .library_top_access(&self.evaluated_characteristics()?, actor, actor)
            .playable)
    }

    pub(super) fn exile_land_play_permitted(
        &self,
        id: ObjectId,
        actor: SeatId,
    ) -> Result<bool, EffectError> {
        let Some(card) = self.window.cards().get(id) else {
            return Ok(false);
        };
        if card.zone() != ObjectZone::Exile {
            return Ok(false);
        }
        let evaluated = self.evaluated_characteristics()?;
        let Some(view) = evaluated
            .get(id)
            .filter(|view| view.has_type(CardType::Land))
        else {
            return Ok(false);
        };
        if self.temporary_rules.iter().any(|active| {
            if active.consumed {
                return false;
            }
            if let crate::program::TemporaryRuleKind::ZoneCastPermission {
                zone,
                filter,
                cast_only: false,
                ..
            } = &active.rule
                && active.resolved_players.contains(&actor)
            {
                let zone_ok = match zone {
                    crate::program::SourceZoneFilter::Exile => true,
                    crate::program::SourceZoneFilter::OpponentExile => card.owner() != actor,
                    crate::program::SourceZoneFilter::LinkedExile => active
                        .source_object
                        .is_some_and(|src| self.is_linked_exile_of(src, id)),
                    _ => false,
                };
                return zone_ok
                    && filter.is_none_or(|f| {
                        self.matches_zone_play_filter(
                            true,
                            f,
                            view,
                            active.controller,
                            active.source_object,
                            &evaluated,
                        )
                    });
            }
            active.controller == actor
                && self
                    .active_temporary_static(active, &evaluated)
                    .is_some_and(|ability| {
                        matches!(
                            ability,
                            crate::characteristics::StaticContinuousAbility::CastPermission {
                                permission:
                                    crate::characteristics::CastPermissionRule::ExileMatchingExt {
                                        filter,
                                        linked_to_source,
                                        lands_allowed: true,
                                        own_turn_only,
                                        ..
                                    },
                                ..
                            } if (!*own_turn_only || self.window.active_player() == actor)
                                && (!*linked_to_source
                                    || active
                                        .source_object
                                        .is_some_and(|src| self.is_linked_exile_of(src, id)))
                                && self.matches_zone_play_filter(
                                    true,
                                    **filter,
                                    view,
                                    actor,
                                    active.source_object,
                                    &evaluated,
                                )
                        )
                    })
        }) {
            return Ok(true);
        }
        if evaluated.emblem_rules.iter().any(|(seat, ability)| {
            *seat == actor
                && matches!(
                    ability,
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission:
                            crate::characteristics::CastPermissionRule::ExileMatchingExt {
                                filter,
                                linked_to_source: false,
                                lands_allowed: true,
                                own_turn_only,
                                ..
                            },
                        ..
                    } if (!*own_turn_only || self.window.active_player() == actor)
                        && self.matches_zone_play_filter(
                            true, **filter, view, actor, None, &evaluated,
                        )
                )
        }) {
            return Ok(true);
        }
        Ok(evaluated.objects.values().any(|source| {
            source.zone == ObjectZone::Battlefield
                && source.controller == Some(actor)
                && source.has_battlefield_rule(|rule| {
                    matches!(
                        rule,
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            id: ability_id,
                            permission:
                                crate::characteristics::CastPermissionRule::ExileMatchingExt {
                                    filter,
                                    linked_to_source,
                                    lands_allowed: true,
                                    once_each_turn,
                                    own_turn_only,
                                    ..
                                },
                        } if (!*own_turn_only || self.window.active_player() == actor)
                            && (!*once_each_turn
                                || !self.source_cast_uses.contains(&(source.object, *ability_id)))
                            && (!*linked_to_source || self.is_linked_exile_of(source.object, id))
                            && self.matches_zone_play_filter(
                                true,
                                **filter,
                                view,
                                actor,
                                Some(source.object),
                                &evaluated,
                            )
                    )
                })
        }))
    }

    pub(super) fn static_exile_cast_grant(
        &self,
        spell: ObjectId,
        actor: SeatId,
        current: &EvaluatedCharacteristics,
    ) -> StaticExileCastGrant {
        if !self
            .window
            .cards()
            .get(spell)
            .is_some_and(|card| card.zone() == ObjectZone::Exile)
        {
            return StaticExileCastGrant::default();
        }
        let Some(spell_view) = current.get(spell) else {
            return StaticExileCastGrant::default();
        };
        let mut unlimited_allowed = false;
        let mut unlimited_free = false;
        let mut unlimited_any_color = false;
        let mut once_candidate: Option<(
            ObjectId,
            crate::characteristics::StaticAbilityId,
            bool,
            bool,
        )> = None;
        let excluded = BTreeSet::new();
        for permanent in current
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield && p.controller == Some(actor))
        {
            for ab in permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
            {
                let active = permanent.abilities.continuous == Some(ab.id())
                    || permanent
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ab.id());
                if !active {
                    continue;
                }
                let matched = match ab {
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        id,
                        permission:
                            crate::characteristics::CastPermissionRule::ExileMatching {
                                filter,
                                linked_to_source,
                                free,
                                any_color,
                            },
                    } => Some((
                        *id,
                        **filter,
                        *linked_to_source,
                        *free,
                        *any_color,
                        false,
                        false,
                    )),
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        id,
                        permission:
                            crate::characteristics::CastPermissionRule::ExileMatchingExt {
                                filter,
                                linked_to_source,
                                free,
                                any_color,
                                once_each_turn,
                                own_turn_only,
                                ..
                            },
                    } => Some((
                        *id,
                        **filter,
                        *linked_to_source,
                        *free,
                        *any_color,
                        *once_each_turn,
                        *own_turn_only,
                    )),
                    _ => None,
                };
                let Some((
                    ability_id,
                    filter,
                    linked_to_source,
                    free,
                    any_color,
                    once_each_turn,
                    own_turn_only,
                )) = matched
                else {
                    continue;
                };
                if own_turn_only && self.window.active_player() != actor {
                    continue;
                }
                if linked_to_source && !self.is_linked_exile_of(permanent.object, spell) {
                    continue;
                }
                if !self.matches_object_filter(
                    filter,
                    spell_view,
                    actor,
                    Some(permanent.object),
                    &excluded,
                    current,
                ) {
                    continue;
                }
                if once_each_turn {
                    if !self
                        .source_cast_uses
                        .contains(&(permanent.object, ability_id))
                        && once_candidate.is_none()
                    {
                        once_candidate = Some((permanent.object, ability_id, free, any_color));
                    }
                } else {
                    unlimited_allowed = true;
                    unlimited_free |= free;
                    unlimited_any_color |= any_color;
                }
            }
        }
        for (seat, ab) in &current.emblem_rules {
            if *seat != actor {
                continue;
            }
            let matched = match ab {
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::ExileMatching {
                            filter,
                            linked_to_source: false,
                            free,
                            any_color,
                        },
                    ..
                } => Some((**filter, *free, *any_color, false)),
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::ExileMatchingExt {
                            filter,
                            linked_to_source: false,
                            free,
                            any_color,
                            own_turn_only,
                            ..
                        },
                    ..
                } => Some((**filter, *free, *any_color, *own_turn_only)),
                _ => None,
            };
            if let Some((filter, free, any_color, own_turn_only)) = matched
                && (!own_turn_only || self.window.active_player() == actor)
                && self.matches_object_filter(filter, spell_view, actor, None, &excluded, current)
            {
                unlimited_allowed = true;
                unlimited_free |= free;
                unlimited_any_color |= any_color;
            }
        }
        for active in &self.temporary_rules {
            if active.controller != actor {
                continue;
            }
            let Some(ab) = self.active_temporary_static(active, current) else {
                continue;
            };
            let matched = match ab {
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::ExileMatching {
                            filter,
                            linked_to_source,
                            free,
                            any_color,
                        },
                    ..
                } => Some((**filter, *linked_to_source, *free, *any_color, false)),
                crate::characteristics::StaticContinuousAbility::CastPermission {
                    permission:
                        crate::characteristics::CastPermissionRule::ExileMatchingExt {
                            filter,
                            linked_to_source,
                            free,
                            any_color,
                            own_turn_only,
                            ..
                        },
                    ..
                } => Some((
                    **filter,
                    *linked_to_source,
                    *free,
                    *any_color,
                    *own_turn_only,
                )),
                _ => None,
            };
            if let Some((filter, linked_to_source, free, any_color, own_turn_only)) = matched
                && (!own_turn_only || self.window.active_player() == actor)
                && (!linked_to_source
                    || active
                        .source_object
                        .is_some_and(|src| self.is_linked_exile_of(src, spell)))
                && self.matches_object_filter(
                    filter,
                    spell_view,
                    actor,
                    active.source_object,
                    &excluded,
                    current,
                )
            {
                unlimited_allowed = true;
                unlimited_free |= free;
                unlimited_any_color |= any_color;
            }
        }
        let allowed = unlimited_allowed || once_candidate.is_some();
        let free = unlimited_free || (!unlimited_allowed && once_candidate.is_some_and(|c| c.2));
        let any_color =
            unlimited_any_color || (!unlimited_allowed && once_candidate.is_some_and(|c| c.3));
        let consumed_once = once_candidate
            .and_then(|(src, ab_id, _, _)| (!unlimited_allowed).then_some((src, ab_id)));
        StaticExileCastGrant {
            allowed,
            free,
            any_color,
            consumed_once,
        }
    }

    pub(super) fn graveyard_land_play_permitted(
        &self,
        id: ObjectId,
        actor: SeatId,
    ) -> Result<bool, EffectError> {
        let Some(card) = self.window.cards().get(id) else {
            return Ok(false);
        };
        if card.zone() == ObjectZone::Exile {
            return self.exile_land_play_permitted(id, actor);
        }
        if card.zone() != ObjectZone::Graveyard(actor) {
            return Ok(false);
        }
        let evaluated = self.evaluated_characteristics()?;
        let Some(view) = evaluated
            .get(id)
            .filter(|view| view.has_type(CardType::Land))
        else {
            return Ok(false);
        };
        if self.card_play_permissions.iter().any(|grant| {
            grant.zone == super::CardPlayZone::Graveyard
                && grant.object == id
                && grant.player == actor
                && !grant.cast_only
        }) {
            return Ok(true);
        }
        if self.temporary_rules.iter().any(|active| {
            if active.consumed {
                return false;
            }
            if let crate::program::TemporaryRuleKind::ZoneCastPermission {
                zone:
                    crate::program::SourceZoneFilter::Graveyard
                    | crate::program::SourceZoneFilter::AnyGraveyard,
                cast_only: false,
                filter,
                ..
            } = &active.rule
                && active.resolved_players.contains(&actor)
            {
                return filter.is_none_or(|f| {
                    self.matches_object_filter(
                        f,
                        view,
                        active.controller,
                        None,
                        &BTreeSet::new(),
                        &evaluated,
                    )
                });
            }
            active.controller == actor
                && self
                    .active_temporary_static(active, &evaluated)
                    .is_some_and(|ability| match ability {
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardLandPlay,
                            ..
                        } => true,
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardMatching {
                                    lands_allowed: true,
                                    filter,
                                    ..
                                },
                            ..
                        } => self.matches_zone_play_filter(
                            true,
                            **filter,
                            view,
                            actor,
                            active.source_object,
                            &evaluated,
                        ),
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            permission:
                                crate::characteristics::CastPermissionRule::GraveyardMatchingOwnTurn {
                                    lands_allowed: true,
                                    filter,
                                    ..
                                },
                            ..
                        } => {
                            self.window.active_player() == actor
                                && self.matches_zone_play_filter(
                                    true,
                                    **filter,
                                    view,
                                    actor,
                                    active.source_object,
                                    &evaluated,
                                )
                        }
                        _ => false,
                    })
        }) {
            return Ok(true);
        }
        if evaluated.emblem_rules.iter().any(|(seat, ability)| {
            *seat == actor
                && match ability {
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission: crate::characteristics::CastPermissionRule::GraveyardLandPlay,
                        ..
                    } => true,
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission:
                            crate::characteristics::CastPermissionRule::GraveyardMatching {
                                lands_allowed: true,
                                filter,
                                ..
                            },
                        ..
                    } => {
                        self.matches_zone_play_filter(true, **filter, view, actor, None, &evaluated)
                    }
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission:
                            crate::characteristics::CastPermissionRule::GraveyardMatchingOwnTurn {
                                lands_allowed: true,
                                filter,
                                ..
                            },
                        ..
                    } => {
                        self.window.active_player() == actor
                            && self.matches_zone_play_filter(
                                true, **filter, view, actor, None, &evaluated,
                            )
                    }
                    _ => false,
                }
        }) {
            return Ok(true);
        }
        Ok(evaluated.objects.values().any(|source| {
            source.zone == ObjectZone::Battlefield
                && source.controller == Some(actor)
                && source.has_battlefield_rule(|rule| match rule {
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission: crate::characteristics::CastPermissionRule::GraveyardLandPlay,
                        ..
                    } => true,
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        id: ability_id,
                        permission:
                            crate::characteristics::CastPermissionRule::GraveyardMatching {
                                lands_allowed: true,
                                filter,
                                once_each_turn,
                            },
                    } => {
                        (!*once_each_turn
                            || !self
                                .source_cast_uses
                                .contains(&(source.object, *ability_id)))
                            && self.matches_zone_play_filter(
                                true,
                                **filter,
                                view,
                                actor,
                                Some(source.object),
                                &evaluated,
                            )
                    }
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        id: ability_id,
                        permission:
                            crate::characteristics::CastPermissionRule::GraveyardMatchingOwnTurn {
                                lands_allowed: true,
                                filter,
                                once_each_turn,
                            },
                    } => {
                        self.window.active_player() == actor
                            && (!*once_each_turn
                                || !self
                                    .source_cast_uses
                                    .contains(&(source.object, *ability_id)))
                            && self.matches_zone_play_filter(
                                true,
                                **filter,
                                view,
                                actor,
                                Some(source.object),
                                &evaluated,
                            )
                    }
                    _ => false,
                })
        }))
    }
}
