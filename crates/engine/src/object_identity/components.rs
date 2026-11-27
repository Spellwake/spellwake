//! Physical components of a merged or melded permanent (CR 713, 730).
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CombinedPermanent {
    /// The first component occupies the normal origin field of GameObject.
    additional: Vec<ObjectOrigin>,
    pub(super) definition: Program,
    // Source-backed top face for display; inherited abilities live in definition.
    top_face: Box<Program>,
    mutations: usize,
    meld_face: Option<[CardId; 2]>,
}

impl GameObject {
    /// Origins in top-to-bottom order. Each component is part of this single permanent.
    pub fn component_origins(&self) -> impl Iterator<Item = &ObjectOrigin> {
        std::iter::once(&self.origin).chain(
            self.combined
                .iter()
                .flat_map(|combined| combined.additional.iter()),
        )
    }

    /// Source face for an effective merged definition. A later copy effect can
    /// independently supply the visible face (CR 613.2a).
    pub fn displayed_face<'a>(&'a self, effective: &'a Program) -> &'a Program {
        self.combined
            .as_ref()
            .filter(|value| &value.definition == effective)
            .map_or(effective, |value| value.top_face.as_ref())
    }

    /// Printed mana value of an actual melded face, including a melded face
    /// that remains on top of additional mutating components (CR 202.3c).
    pub(crate) fn melded_mana_value(
        &self,
        programs: &BTreeMap<CardId, Program>,
    ) -> Option<RulesInteger> {
        let [first, second] = self.combined.as_ref()?.meld_face?;
        Some(
            programs
                .get(&first)?
                .mana_value()
                .plus(&programs.get(&second)?.mana_value()),
        )
    }

    /// Number of successful mutations in this incarnation, independently of copy effects.
    pub fn times_mutated(&self) -> usize {
        self.combined
            .as_ref()
            .map_or(0, |combined| combined.mutations)
    }
}

impl GameObjects {
    /// Merge an already-authorized resolving spell with a battlefield object. The
    /// host keeps its incarnation, controller, tap state and all object-bound work.
    /// `definition` contains the complete copiable result at the merge cut.
    pub(crate) fn merge_mutating_spell(
        &mut self,
        spell: ObjectId,
        host: ObjectId,
        on_top: bool,
        definition: Program,
        top_face: Program,
    ) -> Result<GameObject, ObjectIdentityError> {
        let incoming = self.get(spell).ok_or(ObjectIdentityError::UnknownObject)?;
        let host_object = self.get(host).ok_or(ObjectIdentityError::UnknownObject)?;
        if incoming.zone != ObjectZone::Stack
            || host_object.zone != ObjectZone::Battlefield
            || incoming.owner != host_object.owner
            || definition.permanent().is_none()
        {
            return Err(ObjectIdentityError::InvalidCombination);
        }
        let mut new_host = host_object.clone();
        let origin = match incoming.origin() {
            ObjectOrigin::PermanentSpellCopy { definition } => ObjectOrigin::Token {
                definition: definition.clone(),
            },
            other => other.clone(),
        };
        let mutations = new_host
            .times_mutated()
            .checked_add(1)
            .ok_or(ObjectIdentityError::RevisionExhausted)?;
        let meld_face = (!on_top)
            .then(|| new_host.combined.as_ref().and_then(|v| v.meld_face))
            .flatten();
        let mut additional = new_host
            .combined
            .take()
            .map_or_else(Vec::new, |previous| previous.additional);
        if on_top {
            additional.insert(0, std::mem::replace(&mut new_host.origin, origin));
            new_host.transformed = incoming.transformed;
            new_host.prototype = incoming.prototype;
            new_host.face_down = incoming.face_down;
        } else {
            additional.push(origin);
        }
        new_host.combined = Some(Box::new(CombinedPermanent {
            additional,
            top_face: Box::new(top_face),
            definition,
            mutations,
            meld_face,
        }));
        let new_revision = self.next_revision(self.revision)?;
        let physical = incoming.card_id();
        self.current.remove(&spell);
        self.current.insert(host, new_host.clone());
        if let Some(card) = physical {
            self.by_card.insert(card, host);
        }
        self.revision = new_revision;
        Ok(new_host)
    }

    /// Materialize each underlying component when the single permanent leaves. All
    /// resulting cards and tokens receive new identities and revert to their own faces.
    pub(super) fn separate_components(
        &mut self,
        before: &GameObject,
        top_after: &GameObject,
        destination: ObjectDestination,
    ) -> Result<Vec<ObjectId>, ObjectIdentityError> {
        let Some(combined) = &before.combined else {
            return Ok(Vec::new());
        };
        let mut arrivals = vec![top_after.id];
        for origin in &combined.additional {
            let (id, next_identity) = self.allocate_identity()?;
            let revision = self.next_revision(self.revision)?;
            let mut component = top_after.clone();
            component.id = id;
            component.origin = origin.clone();
            if let Some(card) = component.card_id() {
                self.by_card.insert(card, id);
                self.insert_ordered_zone(id, before.owner, destination);
            }
            self.current.insert(id, component);
            arrivals.push(id);
            self.next_identity = next_identity;
            self.revision = revision;
        }
        Ok(arrivals.into_iter().skip(1).collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn card(n: u64, owner: SeatId, place: ObjectDestination, cards: &mut GameObjects) -> ObjectId {
        let seeded = cards
            .seed_card(cards.revision(), CardId(n), owner, place)
            .unwrap();
        let id = seeded.after.id();
        *cards = seeded.next;
        id
    }
    fn creature() -> Program {
        Program::creature(None, 3.into(), 3.into(), BTreeSet::new())
    }
    #[test]
    fn mutate_keeps_the_host_for_multiple_merges_and_each_component_leaves_once() {
        let a = SeatId(1);
        let mut cards = GameObjects::new(0);
        let host = card(
            1,
            a,
            ObjectDestination::Battlefield { controller: a },
            &mut cards,
        );
        let original_epoch = cards.get(host).unwrap().control_epoch();
        let under = card(2, a, ObjectDestination::Stack { controller: a }, &mut cards);
        let above = card(3, a, ObjectDestination::Stack { controller: a }, &mut cards);
        let mutating = cards
            .merge_mutating_spell(under, host, false, creature(), creature())
            .unwrap();
        assert_eq!(mutating.id(), host);
        let combined = cards
            .merge_mutating_spell(above, host, true, creature(), creature())
            .unwrap();
        assert_eq!(combined.id(), host);
        assert_eq!(combined.times_mutated(), 2);
        assert_eq!(combined.control_epoch(), original_epoch);
        assert_eq!(cards.iter().count(), 1);
        let order: Vec<_> = combined
            .component_origins()
            .map(|origin| match origin {
                ObjectOrigin::Card(card) => card.0,
                _ => panic!("expected a physical card"),
            })
            .collect();
        assert_eq!(order, vec![3, 1, 2]);
        for n in [1, 2, 3] {
            assert_eq!(cards.current_identity(CardId(n)), Some(host));
        }
        let exited = cards
            .apply_zone_move(cards.revision(), host, ObjectDestination::Hand)
            .unwrap()
            .require_moved()
            .unwrap()
            .next;
        let ids: BTreeSet<_> = (1..=3)
            .map(|n| exited.current_identity(CardId(n)).unwrap())
            .collect();
        assert_eq!(ids.len(), 3);
        assert!(!ids.contains(&host));
        assert!(exited.iter().all(|c| c.zone() == ObjectZone::Hand(a)
            && c.times_mutated() == 0
            && c.component_origins().count() == 1));
    }
}
