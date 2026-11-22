//! Continuous visibility and play permissions for the controller's actual library top.
use super::*;
use crate::characteristics::LibraryTopPermission;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LibraryTopAccess {
    pub visible: bool,
    pub playable: bool,
}

impl EffectWindow {
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
        for source in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield && p.controller == Some(owner))
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
                            crate::characteristics::CastPermissionRule::SeeLibraryTop { public },
                        ..
                    } => access.visible |= *public || viewer == owner,
                    crate::characteristics::StaticContinuousAbility::CastPermission {
                        permission:
                            crate::characteristics::CastPermissionRule::PlayLibraryTop(matching),
                        ..
                    } if viewer == owner => {
                        access.playable |= match matching {
                            LibraryTopPermission::Land => card.has_type(CardType::Land),
                            LibraryTopPermission::InstantOrSorcery => {
                                card.has_type(CardType::Instant) || card.has_type(CardType::Sorcery)
                            }
                            LibraryTopPermission::ArtifactOrColorlessSpell => {
                                !card.has_type(CardType::Land)
                                    && (card.has_type(CardType::Artifact)
                                        || card.current_colors.is_empty())
                            }
                            LibraryTopPermission::CreatureOfSourceEntryType => {
                                card.is_creature()
                                    && source.entry_creature_type.is_some_and(|kind| {
                                        card.current_creature_subtypes.contains(&kind)
                                    })
                            }
                        };
                    }
                    _ => {}
                }
            }
        }
        access
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
}
