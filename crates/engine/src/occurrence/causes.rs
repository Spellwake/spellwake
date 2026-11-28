//! Complete owned trigger causes; construction stays with the publication owner.
use super::{EventId, ObjectId, OccurrenceId, SeatId, TriggerError, TriggerRelated};
use crate::program::TriggerGrouping;
use std::collections::{BTreeMap, BTreeSet};

/// One actual match, with its participant captured at the condition's rules cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerCauseMember {
    pub occurrence: OccurrenceId,
    /// All currently represented conditions capture a participant. `None` is
    /// reserved for an explicitly represented condition with no participant role.
    pub related_player: Option<SeatId>,
    pub related: TriggerRelated,
}

impl TriggerCauseMember {
    /// Best-effort subject object identity carried by this trigger cause, if any.
    pub(crate) fn subject_object_id(&self) -> Option<ObjectId> {
        match &self.related {
            TriggerRelated::ZoneChange { after, .. }
            | TriggerRelated::TokenCreated { after, .. }
            | TriggerRelated::Discard { after, .. } => Some(after.object.id()),
            TriggerRelated::Combatant(id)
            | TriggerRelated::AttackedAlone(id)
            | TriggerRelated::TargetedPermanent(id)
            | TriggerRelated::CounterSource(id)
            | TriggerRelated::StateTriggerSource(id)
            | TriggerRelated::Phased(id)
            | TriggerRelated::Mutated(id)
            | TriggerRelated::ManaSource(id)
            | TriggerRelated::MiracleRevealed(id)
            | TriggerRelated::LeftGame(id) => Some(*id),
            TriggerRelated::DamageDealtToPermanent { recipient, .. } => Some(*recipient),
            TriggerRelated::CombatDamageToPlayer { source, .. } => Some(*source),
            TriggerRelated::CombatDamageToPlaneswalker { planeswalker, .. } => Some(*planeswalker),
            TriggerRelated::SubtypeCombatDamage { creature, .. }
            | TriggerRelated::AttachedCreatureDamaged { creature, .. } => Some(*creature),
            TriggerRelated::StatusChange(sc) => Some(sc.after.id()),
            TriggerRelated::SpellCast(cast) => Some(cast.spell().object.id()),
            TriggerRelated::DamageSource(crate::damage::DamageSourceId::Object(id)) => Some(*id),
            TriggerRelated::DeclaredSubtypeAttackers(ids) => ids.first().copied(),
            TriggerRelated::AttackGroup { creatures, .. } => creatures.first().copied(),
            _ => None,
        }
    }
}

/// Ability-specific logical boundary, without a new identity namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerCauseKey {
    Occurrence(OccurrenceId),
    Together,
    Player(SeatId),
}

/// Nonempty, unique, same-event causes in actual occurrence order.
/// Only the native publication owner constructs admitted sets; callers receive
/// immutable views so source departure cannot invalidate retained causality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerCauseSet {
    event: EventId,
    key: TriggerCauseKey,
    members: Vec<TriggerCauseMember>,
}

impl TriggerCauseSet {
    /// Deliberately violate publication invariants for defensive runtime tests.
    /// This mutation is absent from production and cannot qualify a real capture.
    #[cfg(test)]
    pub(crate) fn set_test_members(&mut self, members: Vec<TriggerCauseMember>) {
        self.members = members;
    }

    pub(super) fn new(
        event: EventId,
        key: TriggerCauseKey,
        members: Vec<TriggerCauseMember>,
    ) -> Result<Self, TriggerError> {
        if members.is_empty() {
            return Err(TriggerError::InvalidTriggerCauses);
        }
        let mut seen = BTreeSet::new();
        for member in &members {
            if member.occurrence.event != event || !seen.insert(member.occurrence) {
                return Err(TriggerError::InvalidTriggerCauses);
            }
            if let TriggerCauseKey::Player(player) = key
                && member.related_player != Some(player)
            {
                return Err(TriggerError::InvalidTriggerCauses);
            }
        }
        if let TriggerCauseKey::Occurrence(occurrence) = key
            && (members.len() != 1 || members[0].occurrence != occurrence)
        {
            return Err(TriggerError::InvalidTriggerCauses);
        }
        Ok(Self {
            event,
            key,
            members,
        })
    }

    /// Actual publication shared by every member.
    pub fn event(&self) -> EventId {
        self.event
    }
    /// Logical partition selected by the admitted ability definition.
    pub fn key(&self) -> TriggerCauseKey {
        self.key
    }
    /// Complete matches in their canonical occurrence order.
    pub fn members(&self) -> &[TriggerCauseMember] {
        &self.members
    }
    /// Number of occurrences, independently of distinct objects or players.
    pub fn len(&self) -> usize {
        self.members.len()
    }
    /// Admitted sets are always nonempty.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Partition one complete ordered match list before allocating any instances.
pub(super) fn group_matches(
    event: EventId,
    grouping: TriggerGrouping,
    members: Vec<TriggerCauseMember>,
) -> Result<Vec<TriggerCauseSet>, TriggerError> {
    let mut groups = Vec::new();
    if members.is_empty() {
        return Ok(groups);
    }
    groups
        .try_reserve(members.len())
        .map_err(|_| TriggerError::TriggerCauseCapacity)?;
    match grouping {
        TriggerGrouping::EachOccurrence => {
            for member in members {
                groups.push(TriggerCauseSet::new(
                    event,
                    TriggerCauseKey::Occurrence(member.occurrence),
                    vec![member],
                )?);
            }
        }
        TriggerGrouping::OneOrMoreTogether | TriggerGrouping::OneOrMoreOnceEachTurn => {
            groups.push(TriggerCauseSet::new(
                event,
                TriggerCauseKey::Together,
                members,
            )?);
        }
        TriggerGrouping::OneOrMoreByPlayer => {
            // The map locates groups; only first-match vector order is emitted.
            let mut indices = BTreeMap::new();
            let mut partitions: Vec<(SeatId, Vec<TriggerCauseMember>)> = Vec::new();
            partitions
                .try_reserve(members.len())
                .map_err(|_| TriggerError::TriggerCauseCapacity)?;
            for member in members {
                let player = member
                    .related_player
                    .ok_or(TriggerError::InvalidTriggerCauses)?;
                let index = *indices.entry(player).or_insert_with(|| {
                    let index = partitions.len();
                    partitions.push((player, Vec::new()));
                    index
                });
                let partition = &mut partitions[index].1;
                partition
                    .try_reserve(1)
                    .map_err(|_| TriggerError::TriggerCauseCapacity)?;
                partition.push(member);
            }
            for (player, members) in partitions {
                groups.push(TriggerCauseSet::new(
                    event,
                    TriggerCauseKey::Player(player),
                    members,
                )?);
            }
        }
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::{Step, TurnPosition};

    fn member(event: u64, index: u64, player: u32) -> TriggerCauseMember {
        TriggerCauseMember {
            occurrence: OccurrenceId {
                event: EventId(event),
                index,
            },
            related_player: Some(SeatId(player)),
            related: TriggerRelated::Step {
                position: TurnPosition {
                    number: 3,
                    step: Step::Upkeep,
                    cleanup_iteration: 0,
                    additional_damage: false,
                },
                active: SeatId(player),
            },
        }
    }

    #[test]
    fn grouping_preserves_all_members_and_first_matching_player_order() {
        let members = vec![
            member(4, 0, 2),
            member(4, 1, 0),
            member(4, 2, 2),
            member(4, 3, 1),
        ];
        let each =
            group_matches(EventId(4), TriggerGrouping::EachOccurrence, members.clone()).unwrap();
        assert_eq!(each.len(), 4);
        for (group, member) in each.iter().zip(&members) {
            assert_eq!(group.key(), TriggerCauseKey::Occurrence(member.occurrence));
            assert_eq!(group.members(), std::slice::from_ref(member));
        }
        let together = group_matches(
            EventId(4),
            TriggerGrouping::OneOrMoreTogether,
            members.clone(),
        )
        .unwrap();
        assert_eq!(together.len(), 1);
        assert_eq!(together[0].key(), TriggerCauseKey::Together);
        assert_eq!(together[0].members(), members);
        assert!(!together[0].is_empty());
        let players = group_matches(
            EventId(4),
            TriggerGrouping::OneOrMoreByPlayer,
            members.clone(),
        )
        .unwrap();
        assert_eq!(
            players.iter().map(TriggerCauseSet::key).collect::<Vec<_>>(),
            vec![
                TriggerCauseKey::Player(SeatId(2)),
                TriggerCauseKey::Player(SeatId(0)),
                TriggerCauseKey::Player(SeatId(1))
            ]
        );
        assert_eq!(
            players[0].members(),
            &[members[0].clone(), members[2].clone()]
        );
        assert_eq!(players[1].members(), &[members[1].clone()]);
        assert_eq!(players[2].members(), &[members[3].clone()]);
    }

    #[test]
    fn empty_matches_never_create_a_group_for_any_policy() {
        for policy in [
            TriggerGrouping::EachOccurrence,
            TriggerGrouping::OneOrMoreTogether,
            TriggerGrouping::OneOrMoreByPlayer,
        ] {
            assert!(
                group_matches(EventId(1), policy, vec![])
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn checked_sets_reject_empty_foreign_duplicate_and_inconsistent_keys() {
        let first = member(1, 0, 2);
        let second = member(1, 1, 2);
        let mut roleless = first.clone();
        roleless.related_player = None;
        for (key, members) in [
            (TriggerCauseKey::Together, vec![]),
            (
                TriggerCauseKey::Together,
                vec![first.clone(), member(2, 1, 2)],
            ),
            (
                TriggerCauseKey::Together,
                vec![first.clone(), first.clone()],
            ),
            (
                TriggerCauseKey::Occurrence(second.occurrence),
                vec![first.clone()],
            ),
            (
                TriggerCauseKey::Occurrence(first.occurrence),
                vec![first.clone(), second],
            ),
            (TriggerCauseKey::Player(SeatId(0)), vec![first.clone()]),
            (TriggerCauseKey::Player(SeatId(2)), vec![roleless.clone()]),
        ] {
            assert_eq!(
                TriggerCauseSet::new(EventId(1), key, members),
                Err(TriggerError::InvalidTriggerCauses)
            );
        }
        assert_eq!(
            group_matches(
                EventId(1),
                TriggerGrouping::OneOrMoreByPlayer,
                vec![roleless]
            ),
            Err(TriggerError::InvalidTriggerCauses)
        );
        assert!(
            TriggerCauseSet::new(
                EventId(1),
                TriggerCauseKey::Occurrence(first.occurrence),
                vec![first]
            )
            .is_ok()
        );
    }
}
