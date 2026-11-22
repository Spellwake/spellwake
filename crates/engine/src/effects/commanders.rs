//! Commander designations belong to physical cards, including after a zone change (CR 903.3).
use super::*;

/// Independent cast count and combat damage dealt by one designated card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommanderRecord {
    pub(super) return_eligible: Option<ObjectId>,
    command_zone_casts: RulesInteger,
    combat_damage: BTreeMap<SeatId, RulesInteger>,
}
impl Default for CommanderRecord {
    fn default() -> Self {
        Self {
            return_eligible: None,
            command_zone_casts: 0.into(),
            combat_damage: BTreeMap::new(),
        }
    }
}
impl CommanderRecord {
    pub(super) fn record_cast(&mut self) {
        self.command_zone_casts = self.command_zone_casts.plus(&1.into());
    }
    pub fn command_zone_casts(&self) -> &RulesInteger {
        &self.command_zone_casts
    }
    pub fn combat_damage(&self) -> &BTreeMap<SeatId, RulesInteger> {
        &self.combat_damage
    }
}
impl EffectWindow {
    pub(crate) fn designate_commanders(&mut self, designated: BTreeSet<CardId>) {
        self.characteristics.commander_cards = designated.clone();
        self.commanders = designated
            .into_iter()
            .map(|card| (card, CommanderRecord::default()))
            .collect();
    }
    pub(crate) fn set_commander_color_identities(
        &mut self,
        identities: BTreeMap<SeatId, BTreeSet<crate::characteristics::Color>>,
    ) {
        self.commander_color_identities = Some(identities);
    }
    pub(super) fn commander_mana_colors(
        &self,
        player: SeatId,
    ) -> Result<BTreeSet<ManaType>, EffectError> {
        use crate::characteristics::Color;
        let Some(identities) = &self.commander_color_identities else {
            let owns_commander = self.window.cards().iter().any(|object| {
                object.owner() == player
                    && object
                        .card_id()
                        .is_some_and(|id| self.commanders.contains_key(&id))
            });
            return if owns_commander {
                Err(EffectError::MissingCommanderColorIdentity)
            } else {
                Ok(BTreeSet::new())
            };
        };
        let colors = identities
            .get(&player)
            .ok_or(EffectError::MissingCommanderColorIdentity)?;
        Ok(colors
            .iter()
            .map(|color| match color {
                Color::White => ManaType::White,
                Color::Blue => ManaType::Blue,
                Color::Black => ManaType::Black,
                Color::Red => ManaType::Red,
                Color::Green => ManaType::Green,
            })
            .collect())
    }
    /// Pregame color chosen for this physical commander, when its definition requires one.
    pub fn commander_chosen_color(&self, card: CardId) -> Option<crate::characteristics::Color> {
        self.commanders.get(&card).and_then(|_| {
            self.characteristics
                .commander_chosen_colors
                .get(&card)
                .copied()
        })
    }
    /// Physical designation, recorded casts, combat damage, and current return eligibility.
    pub fn commander_record(&self, card: CardId) -> Option<&CommanderRecord> {
        self.commanders.get(&card)
    }
    pub(super) fn commander_in_command(
        &self,
        object: &GameObject,
        actor: SeatId,
    ) -> Option<CardId> {
        let id = object.card_id()?;
        (object.zone() == ObjectZone::Command
            && object.owner() == actor
            && self.commanders.contains_key(&id))
        .then_some(id)
    }
    pub(super) fn commander_damage_losses(&self) -> impl Iterator<Item = (SeatId, CardId)> + '_ {
        self.commanders.iter().flat_map(|(&card, record)| {
            record
                .combat_damage
                .iter()
                .filter(|(_, n)| **n >= 21.into())
                .map(move |(&seat, _)| (seat, card))
        })
    }
    pub(super) fn record_commander_damage(
        &mut self,
        source: &DamageSource,
        player: SeatId,
        amount: &RulesInteger,
    ) {
        let DamageSource::Object { object, .. } = source else {
            return;
        };
        if let Some(record) = object.card_id().and_then(|id| self.commanders.get_mut(&id)) {
            let dealt = record
                .combat_damage
                .entry(player)
                .or_insert_with(|| 0.into());
            *dealt = dealt.plus(amount);
        }
    }
}

#[cfg(test)]
mod mana_identity_tests {
    use super::*;
    use crate::characteristics::Color;
    use crate::mana::ManaType;

    #[test]
    fn identity_distinguishes_colorless_commander_missing_input_and_no_commander() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![SeedCard {
                card: CardId(1),
                owner: a,
                destination: ObjectDestination::Command,
                program: Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            }],
            vec![],
            vec![],
            40.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        assert!(state.commander_mana_colors(a).unwrap().is_empty());
        state.designate_commanders(BTreeSet::from([CardId(1)]));
        assert_eq!(
            state.commander_mana_colors(a),
            Err(EffectError::MissingCommanderColorIdentity)
        );
        assert!(state.commander_mana_colors(b).unwrap().is_empty());
        state.set_commander_color_identities(BTreeMap::from([
            (a, BTreeSet::new()),
            (b, BTreeSet::from([Color::White, Color::Blue])),
        ]));
        assert!(state.commander_mana_colors(a).unwrap().is_empty());
        assert_eq!(
            state.commander_mana_colors(b).unwrap(),
            BTreeSet::from([ManaType::White, ManaType::Blue])
        );
    }
}
