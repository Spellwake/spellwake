//! Preserve spend effects from production through a particular payment (CR 106.6).
use super::*;
use crate::mana::{FloatingManaLot, ManaSpendEffect, RestrictedManaSpend};
use crate::occurrence::{RulesOccurrence, SupportedAppearance};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Lot {
    metadata: FloatingManaLot,
    source: Box<SupportedAppearance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManaSourceTraitCounts {
    pub(super) snow: RulesInteger,
    pub(super) treasure: RulesInteger,
    pub(super) creature: RulesInteger,
}
impl Default for ManaSourceTraitCounts {
    fn default() -> Self {
        Self {
            snow: 0.into(),
            treasure: 0.into(),
            creature: 0.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ManaProvenance {
    pub(super) next_id: u64,
    lots: BTreeMap<u64, Lot>,
    commander_entries: BTreeMap<ObjectId, RulesInteger>,
    cast_spending: BTreeMap<StackEntryId, Vec<SpentManaLot>>,
    pub(super) source_traits: BTreeMap<(SeatId, ManaType), ManaSourceTraitCounts>,
}

pub(super) type SpentManaLot = (ManaSpendEffect, Box<SupportedAppearance>, RulesInteger);

impl ManaProvenance {
    pub(super) fn remember_cast(&mut self, entry: StackEntryId, spent: Vec<SpentManaLot>) {
        if !spent.is_empty() {
            self.cast_spending.insert(entry, spent);
        }
    }
    pub(super) fn end_combat_effects(&mut self) {
        self.lots
            .retain(|_, lot| lot.metadata.effect != ManaSpendEffect::RetainUntilEndOfCombat);
    }
    pub(super) fn end_turn_effects(&mut self) {
        // These units become ordinary until the cleanup step ends.
        self.lots.retain(|_, lot| {
            !matches!(
                lot.metadata.effect,
                ManaSpendEffect::RetainUntilCleanup | ManaSpendEffect::RetainUntilEndOfCombat
            )
        });
    }
}

impl EffectWindow {
    pub(super) fn expire_step_mana(&mut self, retained: &BTreeSet<(SeatId, ManaType)>) {
        let converted = self
            .evaluated_characteristics()
            .map(|evaluated| self.converted_unspent_mana_types(&evaluated))
            .unwrap_or_default();
        self.expire_step_mana_with_conversion(retained, &converted);
    }

    pub(super) fn expire_step_mana_with_conversion(
        &mut self,
        retained: &BTreeSet<(SeatId, ManaType)>,
        converted: &BTreeMap<SeatId, ManaType>,
    ) {
        let mut retained_units: BTreeMap<(SeatId, ManaType), RulesInteger> = BTreeMap::new();
        self.floating_mana.lots.retain(|_, lot| {
            let key = (lot.metadata.player, lot.metadata.mana_type);
            if retained.contains(&key) {
                return true;
            }
            if matches!(
                lot.metadata.effect,
                ManaSpendEffect::RetainUntilCleanup | ManaSpendEffect::RetainUntilEndOfCombat
            ) {
                let total = retained_units
                    .entry(key)
                    .or_insert_with(|| RulesInteger::from(0));
                *total = total.plus(&lot.metadata.remaining);
                return true;
            }
            if let Some(&target) = converted.get(&lot.metadata.player) {
                lot.metadata.mana_type = target;
                return true;
            }
            false
        });
        let mut converted_additions: BTreeMap<(SeatId, ManaType), RulesInteger> = BTreeMap::new();
        let mut converted_keys: BTreeMap<(SeatId, ManaType), ManaType> = BTreeMap::new();
        self.mana.retain(|&key, amount| {
            if retained.contains(&key) {
                return true;
            }
            let (player, kind) = key;
            let keep_lots = retained_units.get(&key);
            if let Some(&target) = converted.get(&player) {
                if kind == target {
                    return amount.is_positive();
                }
                if let Some(count) = keep_lots {
                    let excess = amount.minus(count);
                    if excess.is_positive() {
                        let entry = converted_additions
                            .entry((player, target))
                            .or_insert_with(|| RulesInteger::from(0));
                        *entry = entry.plus(&excess);
                        converted_keys.insert(key, target);
                    }
                    *amount = count.clone();
                    return amount.is_positive();
                }
                if amount.is_positive() {
                    let entry = converted_additions
                        .entry((player, target))
                        .or_insert_with(|| RulesInteger::from(0));
                    *entry = entry.plus(amount);
                    converted_keys.insert(key, target);
                }
                return false;
            }
            if let Some(count) = keep_lots {
                *amount = count.clone();
                return true;
            }
            false
        });
        for (key, added) in converted_additions {
            if added.is_positive() {
                let entry = self
                    .mana
                    .entry(key)
                    .or_insert_with(|| RulesInteger::from(0));
                *entry = entry.plus(&added);
            }
        }
        if converted.is_empty() {
            self.restricted_mana
                .retain(|(player, kind, _), _| retained.contains(&(*player, *kind)));
        } else {
            let old_restricted = std::mem::take(&mut self.restricted_mana);
            for ((player, kind, restriction), amount) in old_restricted {
                if !amount.is_positive() {
                    continue;
                }
                if retained.contains(&(player, kind)) {
                    let entry = self
                        .restricted_mana
                        .entry((player, kind, restriction))
                        .or_insert_with(|| RulesInteger::from(0));
                    *entry = entry.plus(&amount);
                } else if let Some(&target) = converted.get(&player) {
                    let entry = self
                        .restricted_mana
                        .entry((player, target, restriction))
                        .or_insert_with(|| RulesInteger::from(0));
                    *entry = entry.plus(&amount);
                }
            }
        }
        if !converted_keys.is_empty() {
            let old_traits = std::mem::take(&mut self.floating_mana.source_traits);
            for (key, counts) in old_traits {
                if let Some(&target) = converted_keys.get(&key) {
                    let kept_cap = self
                        .mana
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| RulesInteger::from(0));
                    let keep_snow = counts.snow.clone().min(kept_cap.clone());
                    let keep_treasure = counts.treasure.clone().min(kept_cap.clone());
                    let keep_creature = counts.creature.clone().min(kept_cap);
                    if keep_snow.is_positive()
                        || keep_treasure.is_positive()
                        || keep_creature.is_positive()
                    {
                        let kept_entry = self.floating_mana.source_traits.entry(key).or_default();
                        kept_entry.snow = kept_entry.snow.plus(&keep_snow);
                        kept_entry.treasure = kept_entry.treasure.plus(&keep_treasure);
                        kept_entry.creature = kept_entry.creature.plus(&keep_creature);
                    }
                    let move_snow = counts.snow.minus(&keep_snow);
                    let move_treasure = counts.treasure.minus(&keep_treasure);
                    let move_creature = counts.creature.minus(&keep_creature);
                    if move_snow.is_positive()
                        || move_treasure.is_positive()
                        || move_creature.is_positive()
                    {
                        let dest_entry = self
                            .floating_mana
                            .source_traits
                            .entry((key.0, target))
                            .or_default();
                        dest_entry.snow = dest_entry.snow.plus(&move_snow);
                        dest_entry.treasure = dest_entry.treasure.plus(&move_treasure);
                        dest_entry.creature = dest_entry.creature.plus(&move_creature);
                    }
                } else {
                    let entry = self.floating_mana.source_traits.entry(key).or_default();
                    entry.snow = entry.snow.plus(&counts.snow);
                    entry.treasure = entry.treasure.plus(&counts.treasure);
                    entry.creature = entry.creature.plus(&counts.creature);
                }
            }
        }
        self.floating_mana.source_traits.retain(|key, counts| {
            let Some(remaining) = self.mana.get(key) else {
                return false;
            };
            counts.snow = counts.snow.clone().min(remaining.clone());
            counts.treasure = counts.treasure.clone().min(remaining.clone());
            counts.creature = counts.creature.clone().min(remaining.clone());
            counts.snow.is_positive()
                || counts.treasure.is_positive()
                || counts.creature.is_positive()
        });
    }

    pub(super) fn record_mana_source_traits(
        &mut self,
        owner: SeatId,
        produced: &BTreeMap<ManaType, RulesInteger>,
        is_snow: bool,
        is_treasure: bool,
        is_creature: bool,
    ) {
        if !is_snow && !is_treasure && !is_creature {
            return;
        }
        for (&mana_type, amount) in produced.iter().filter(|(_, amount)| amount.is_positive()) {
            let entry = self
                .floating_mana
                .source_traits
                .entry((owner, mana_type))
                .or_default();
            if is_snow {
                entry.snow = entry.snow.plus(amount);
            }
            if is_treasure {
                entry.treasure = entry.treasure.plus(amount);
            }
            if is_creature {
                entry.creature = entry.creature.plus(amount);
            }
        }
    }

    pub(super) fn consume_mana_source_traits(
        &mut self,
        player: SeatId,
        spend: &[ManaSpend],
    ) -> (RulesInteger, RulesInteger, RulesInteger) {
        let mut snow = RulesInteger::from(0);
        let mut treasure = RulesInteger::from(0);
        let mut creature = RulesInteger::from(0);
        for unit in spend.iter().filter(|u| u.amount.is_positive()) {
            if let Some(entry) = self
                .floating_mana
                .source_traits
                .get_mut(&(player, unit.mana_type))
            {
                let take_snow = entry.snow.clone().min(unit.amount.clone());
                if take_snow.is_positive() {
                    entry.snow = entry.snow.minus(&take_snow);
                    snow = snow.plus(&take_snow);
                }
                let take_treasure = entry.treasure.clone().min(unit.amount.clone());
                if take_treasure.is_positive() {
                    entry.treasure = entry.treasure.minus(&take_treasure);
                    treasure = treasure.plus(&take_treasure);
                }
                let take_creature = entry.creature.clone().min(unit.amount.clone());
                if take_creature.is_positive() {
                    entry.creature = entry.creature.minus(&take_creature);
                    creature = creature.plus(&take_creature);
                }
            }
        }
        self.floating_mana.source_traits.retain(|_, counts| {
            counts.snow.is_positive()
                || counts.treasure.is_positive()
                || counts.creature.is_positive()
        });
        (snow, treasure, creature)
    }

    /// Mana carrying additional effects. These units can pay any ordinary mana cost.
    pub fn floating_mana_lots(&self) -> impl Iterator<Item = (u64, &FloatingManaLot)> {
        self.floating_mana
            .lots
            .iter()
            .map(|(&id, lot)| (id, &lot.metadata))
    }

    pub(super) fn record_mana_lots(
        &mut self,
        source: SupportedAppearance,
        owner: SeatId,
        produced: &BTreeMap<ManaType, RulesInteger>,
        effect: ManaSpendEffect,
    ) -> Result<(), EffectError> {
        for (&mana_type, amount) in produced.iter().filter(|(_, amount)| amount.is_positive()) {
            let id = self
                .floating_mana
                .next_id
                .checked_add(1)
                .ok_or(EffectError::InconsistentProgram)?;
            self.floating_mana.next_id = id;
            self.floating_mana.lots.insert(
                id,
                Lot {
                    metadata: FloatingManaLot {
                        player: owner,
                        source: source.object.id(),
                        mana_type,
                        effect,
                        remaining: amount.clone(),
                    },
                    source: Box::new(source.clone()),
                },
            );
        }
        Ok(())
    }

    /// First honor the payer's selections. Fill the remainder from ordinary units, then
    /// from eligible lots in production order. Restricted units have a separate allocation.
    pub(super) fn spend_mana_lots(
        &mut self,
        player: SeatId,
        spend: &[ManaSpend],
        restricted: &[RestrictedManaSpend],
        preferences: Option<&BTreeMap<u64, RulesInteger>>,
    ) -> Result<Vec<SpentManaLot>, EffectError> {
        if self.floating_mana.lots.is_empty() && preferences.is_none_or(BTreeMap::is_empty) {
            return Ok(Vec::new());
        }
        let invalid = || EffectError::InvalidManaPayment(ManaPaymentError::WrongTotal);
        let mut selected = preferences.cloned().unwrap_or_default();
        for (&id, count) in &selected {
            if !count.is_positive()
                || self.floating_mana.lots.get(&id).is_none_or(|lot| {
                    lot.metadata.player != player
                        || lot.metadata.remaining < *count
                        || !spend.iter().any(|s| s.mana_type == lot.metadata.mana_type)
                })
            {
                return Err(invalid());
            }
        }
        for paid in spend {
            let lots = self.floating_mana.lots.iter().filter(|(_, lot)| {
                lot.metadata.player == player && lot.metadata.mana_type == paid.mana_type
            });
            let total = lots.clone().fold(RulesInteger::from(0), |sum, (_, lot)| {
                sum.plus(&lot.metadata.remaining)
            });
            let explicit = lots
                .clone()
                .filter_map(|(id, _)| selected.get(id))
                .fold(RulesInteger::from(0), |sum, n| sum.plus(n));
            let restricted_paid = restricted
                .iter()
                .filter(|r| r.mana_type == paid.mana_type)
                .fold(RulesInteger::from(0), |sum, r| sum.plus(&r.amount));
            if explicit.plus(&restricted_paid) > paid.amount {
                return Err(invalid());
            }
            // Restricted units for this payment were released from their restricted pool before this call.
            let ordinary = self.unrestricted_mana(player, paid.mana_type).minus(&total);
            let mut missing = paid.amount.minus(&explicit).minus(&ordinary);
            for (&id, lot) in lots {
                if !missing.is_positive() {
                    break;
                }
                let selected = selected.entry(id).or_insert_with(|| 0.into());
                let take = missing.clone().min(lot.metadata.remaining.minus(selected));
                *selected = selected.plus(&take);
                missing = missing.minus(&take);
            }
            if missing.is_positive() {
                return Err(invalid());
            }
        }
        let mut spent = Vec::new();
        for (id, count) in selected
            .into_iter()
            .filter(|(_, count)| count.is_positive())
        {
            let lot = self.floating_mana.lots.get_mut(&id).ok_or_else(invalid)?;
            lot.metadata.remaining = lot.metadata.remaining.minus(&count);
            spent.push((lot.metadata.effect, lot.source.clone(), count));
            if !lot.metadata.remaining.is_positive() {
                self.floating_mana.lots.remove(&id);
            }
        }
        Ok(spent)
    }

    pub(super) fn mana_spend_effects(
        &mut self,
        entry: StackEntryId,
        payer: SeatId,
        view: &EvaluatedCharacteristics,
    ) -> Result<Vec<RulesOccurrence>, EffectError> {
        let Some(spent) = self.floating_mana.cast_spending.remove(&entry) else {
            return Ok(Vec::new());
        };
        let Some(StackKind::CardSpell(object)) = self
            .window
            .entries()
            .iter()
            .find(|e| e.id() == entry)
            .map(|e| e.kind())
        else {
            return Ok(Vec::new());
        };
        let object = *object;
        let spell = view.get(object).ok_or(EffectError::InconsistentProgram)?;
        let commanders = self.window.cards().iter().filter(|card| {
            card.owner() == payer
                && card
                    .card_id()
                    .is_some_and(|id| self.commanders.contains_key(&id))
        });
        let is_commander = self.window.cards().get(object).is_some_and(|card| {
            card.owner() == payer
                && card
                    .card_id()
                    .is_some_and(|id| self.commanders.contains_key(&id))
        });
        // CR 903.3e makes commanders discoverable even in hand and library. A face-down
        // card in exile has no characteristics. Phased-out permanents are absent from cards().iter().
        let shared = spell.is_creature()
            && commanders
                .filter(|card| {
                    card.zone() != ObjectZone::Exile
                        || (self.face_down_exile_viewer(card.id()).is_none()
                            && self.foretold_on(card.id()).is_none())
                })
                .filter_map(|card| view.get(card.id()))
                .any(|commander| {
                    !commander
                        .current_creature_subtypes
                        .is_disjoint(&spell.current_creature_subtypes)
                });
        let mut events = Vec::new();
        for (effect, source, count) in spent {
            match effect {
                ManaSpendEffect::CommanderEntryCounters if is_commander => {
                    let amount = self
                        .floating_mana
                        .commander_entries
                        .entry(object)
                        .or_insert_with(|| 0.into());
                    *amount = amount.plus(&count);
                }
                ManaSpendEffect::SharedCommanderTypeScry if shared => {
                    events.push(RulesOccurrence::CommanderTypeManaSpent {
                        source,
                        player: payer,
                        units: count,
                    });
                }
                _ => {}
            }
        }
        Ok(events)
    }

    pub(super) fn take_commander_mana_entry_counters(
        &mut self,
        spell: ObjectId,
        card: CardId,
    ) -> RulesInteger {
        self.floating_mana
            .commander_entries
            .remove(&spell)
            .and_then(|mana| {
                self.commanders
                    .get(&card)
                    .map(|commander| mana.times(commander.command_zone_casts()))
            })
            .unwrap_or_else(|| 0.into())
    }
    pub(super) fn forget_departed_commander_mana(&mut self, spell: ObjectId) {
        self.floating_mana.commander_entries.remove(&spell);
    }
}
