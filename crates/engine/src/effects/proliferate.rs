//! Choose any subset of counter-bearing battlefield permanents and players (CR 701).
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

/// Counter kinds that can occur on a permanent or player in this engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CounterKind {
    PlusOne,
    MinusOne,
    Page,
    Quest,
    Verse,
    Suspect,
    Charge,
    Storage,
    Lore,
    Loyalty,
    Defense,
    Time,
    Fade,
    Age,
    Indestructible,
    Corpse,
    Vow,
    Blessing,
    Despair,
    Hatchling,
    Study,
    Eyeball,
    Slime,
    Oil,
    Level,
    Spore,
    Ki,
    Soul,
    Fuse,
    Depletion,
    Bounty,
    Flood,
    Flame,
    Hour,
    Brick,
    Plague,
    Doom,
    Pressure,
    Feather,
    Fate,
    Infection,
    Ice,
    Tide,
    Growth,
    Divinity,
    Fungus,
    Hit,
    Gold,
    Blood,
    Cage,
    Coin,
    Credit,
    Death,
    Delay,
    Devotion,
    Egg,
    Eon,
    Fury,
    Incubation,
    Knowledge,
    Luck,
    Mine,
    Music,
    Muster,
    Net,
    Omen,
    Ore,
    Pain,
    Petal,
    Phylactery,
    Pin,
    Plot,
    Prey,
    Pupa,
    Shell,
    Silver,
    Sleep,
    Soot,
    Stash,
    Strife,
    Task,
    Tower,
    Training,
    Trap,
    Treasure,
    Valor,
    Velocity,
    Vitality,
    Void,
    Vortex,
    Wage,
    Winch,
    Wind,
    Wish,
    /// CR 122.1b keyword counters.
    Flying,
    FirstStrike,
    DoubleStrike,
    Deathtouch,
    Haste,
    Hexproof,
    Lifelink,
    Menace,
    Reach,
    Shadow,
    Trample,
    Vigilance,
    /// CR 122.1d stun counter (replaces untapping with removing one stun counter).
    Stun,
    /// CR 122.1c shield counter (replaces destruction or prevents damage by removing one shield counter).
    Shield,
    /// CR 122.1h finality counter (exiles a permanent instead of putting it into a graveyard).
    Finality,
    Poison,
    Experience,
    Energy,
}

impl CounterKind {
    /// The counter kinds representable on battlefield permanents.
    pub const PERMANENT: [Self; 109] = {
        use CounterKind::*;
        [
            PlusOne,
            MinusOne,
            Page,
            Quest,
            Verse,
            Suspect,
            Charge,
            Storage,
            Lore,
            Loyalty,
            Defense,
            Time,
            Fade,
            Age,
            Indestructible,
            Corpse,
            Vow,
            Blessing,
            Despair,
            Hatchling,
            Study,
            Eyeball,
            Slime,
            Oil,
            Level,
            Spore,
            Ki,
            Soul,
            Fuse,
            Depletion,
            Bounty,
            Flood,
            Flame,
            Hour,
            Brick,
            Plague,
            Doom,
            Pressure,
            Feather,
            Fate,
            Infection,
            Ice,
            Tide,
            Growth,
            Divinity,
            Fungus,
            Hit,
            Gold,
            Blood,
            Cage,
            Coin,
            Credit,
            Death,
            Delay,
            Devotion,
            Egg,
            Eon,
            Fury,
            Incubation,
            Knowledge,
            Luck,
            Mine,
            Music,
            Muster,
            Net,
            Omen,
            Ore,
            Pain,
            Petal,
            Phylactery,
            Pin,
            Plot,
            Prey,
            Pupa,
            Shell,
            Silver,
            Sleep,
            Soot,
            Stash,
            Strife,
            Task,
            Tower,
            Training,
            Trap,
            Treasure,
            Valor,
            Velocity,
            Vitality,
            Void,
            Vortex,
            Wage,
            Winch,
            Wind,
            Wish,
            Flying,
            FirstStrike,
            DoubleStrike,
            Deathtouch,
            Haste,
            Hexproof,
            Lifelink,
            Menace,
            Reach,
            Shadow,
            Trample,
            Vigilance,
            Stun,
            Shield,
            Finality,
        ]
    };

    /// Keyword ability granted by a CR 122.1b keyword counter on an object.
    #[must_use]
    pub const fn keyword_counter_ability(self) -> Option<crate::characteristics::KeywordAbility> {
        use crate::characteristics::KeywordAbility;
        Some(match self {
            Self::Indestructible => KeywordAbility::Indestructible,
            Self::Flying => KeywordAbility::Flying,
            Self::FirstStrike => KeywordAbility::FirstStrike,
            Self::DoubleStrike => KeywordAbility::DoubleStrike,
            Self::Deathtouch => KeywordAbility::Deathtouch,
            Self::Haste => KeywordAbility::Haste,
            Self::Hexproof => KeywordAbility::Hexproof,
            Self::Lifelink => KeywordAbility::Lifelink,
            Self::Menace => KeywordAbility::Menace,
            Self::Reach => KeywordAbility::Reach,
            Self::Shadow => KeywordAbility::Shadow,
            Self::Trample => KeywordAbility::Trample,
            Self::Vigilance => KeywordAbility::Vigilance,
            _ => return None,
        })
    }

    /// Whether this counter kind is a player counter (CR 122.1).
    #[must_use]
    pub const fn is_player_counter(self) -> bool {
        matches!(self, Self::Poison | Self::Experience | Self::Energy)
    }

    pub(crate) fn is_named(self) -> bool {
        !self.is_player_counter() && !matches!(self, Self::PlusOne | Self::MinusOne)
    }

    pub(crate) fn tracks_linked_values(self) -> bool {
        matches!(self, Self::Study | Self::Eyeball | Self::Slime)
    }

    /// Resolve a lowercase Oracle counter-kind label (for example `"+1/+1"`, `"charge"`, `"quest"`).
    #[must_use]
    pub fn from_oracle_label(label: &str) -> Option<Self> {
        Some(match label {
            "+1/+1" => Self::PlusOne,
            "-1/-1" => Self::MinusOne,
            "page" => Self::Page,
            "quest" => Self::Quest,
            "verse" => Self::Verse,
            "suspect" => Self::Suspect,
            "charge" => Self::Charge,
            "storage" => Self::Storage,
            "lore" => Self::Lore,
            "loyalty" => Self::Loyalty,
            "defense" => Self::Defense,
            "time" => Self::Time,
            "fade" => Self::Fade,
            "age" => Self::Age,
            "indestructible" => Self::Indestructible,
            "corpse" => Self::Corpse,
            "vow" => Self::Vow,
            "blessing" => Self::Blessing,
            "despair" => Self::Despair,
            "hatchling" => Self::Hatchling,
            "study" => Self::Study,
            "eyeball" => Self::Eyeball,
            "slime" => Self::Slime,
            "oil" => Self::Oil,
            "level" => Self::Level,
            "spore" => Self::Spore,
            "ki" => Self::Ki,
            "soul" => Self::Soul,
            "fuse" => Self::Fuse,
            "depletion" => Self::Depletion,
            "bounty" => Self::Bounty,
            "flood" => Self::Flood,
            "flame" => Self::Flame,
            "hour" => Self::Hour,
            "brick" => Self::Brick,
            "plague" => Self::Plague,
            "doom" => Self::Doom,
            "pressure" => Self::Pressure,
            "feather" => Self::Feather,
            "fate" => Self::Fate,
            "infection" => Self::Infection,
            "ice" => Self::Ice,
            "tide" => Self::Tide,
            "growth" => Self::Growth,
            "divinity" => Self::Divinity,
            "fungus" => Self::Fungus,
            "hit" => Self::Hit,
            "gold" => Self::Gold,
            "blood" => Self::Blood,
            "cage" => Self::Cage,
            "coin" => Self::Coin,
            "credit" => Self::Credit,
            "death" => Self::Death,
            "delay" => Self::Delay,
            "devotion" => Self::Devotion,
            "egg" => Self::Egg,
            "eon" => Self::Eon,
            "fury" => Self::Fury,
            "incubation" => Self::Incubation,
            "knowledge" => Self::Knowledge,
            "luck" => Self::Luck,
            "mine" => Self::Mine,
            "music" => Self::Music,
            "muster" => Self::Muster,
            "net" => Self::Net,
            "omen" => Self::Omen,
            "ore" => Self::Ore,
            "pain" => Self::Pain,
            "petal" => Self::Petal,
            "phylactery" => Self::Phylactery,
            "pin" => Self::Pin,
            "plot" => Self::Plot,
            "prey" => Self::Prey,
            "pupa" => Self::Pupa,
            "shell" => Self::Shell,
            "silver" => Self::Silver,
            "sleep" => Self::Sleep,
            "soot" => Self::Soot,
            "stash" => Self::Stash,
            "strife" => Self::Strife,
            "task" => Self::Task,
            "tower" => Self::Tower,
            "training" => Self::Training,
            "trap" => Self::Trap,
            "treasure" => Self::Treasure,
            "valor" => Self::Valor,
            "velocity" => Self::Velocity,
            "vitality" => Self::Vitality,
            "void" => Self::Void,
            "vortex" => Self::Vortex,
            "wage" => Self::Wage,
            "winch" => Self::Winch,
            "wind" => Self::Wind,
            "wish" => Self::Wish,
            "flying" => Self::Flying,
            "first strike" => Self::FirstStrike,
            "double strike" => Self::DoubleStrike,
            "deathtouch" => Self::Deathtouch,
            "haste" => Self::Haste,
            "hexproof" => Self::Hexproof,
            "lifelink" => Self::Lifelink,
            "menace" => Self::Menace,
            "reach" => Self::Reach,
            "shadow" => Self::Shadow,
            "trample" => Self::Trample,
            "vigilance" => Self::Vigilance,
            "stun" => Self::Stun,
            "shield" => Self::Shield,
            "finality" => Self::Finality,
            "poison" => Self::Poison,
            "experience" => Self::Experience,
            "energy" => Self::Energy,
            _ => return None,
        })
    }
}

/// Resolution-time non-targeted choices, bound to an exact source instruction and revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProliferateChoice {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    domain: BTreeMap<TargetRef, BTreeSet<CounterKind>>,
}
impl ProliferateChoice {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn domain(&self) -> &BTreeMap<TargetRef, BTreeSet<CounterKind>> {
        &self.domain
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
impl CharacteristicState {
    pub(crate) fn total_permanent_counters(&self, object: ObjectId) -> RulesInteger {
        CounterKind::PERMANENT
            .into_iter()
            .fold(RulesInteger::from(0), |total, kind| {
                total.plus(&self.permanent_counter_count(object, kind))
            })
    }
    pub(crate) fn named_permanent_counters_mut(
        &mut self,
        kind: CounterKind,
    ) -> Option<&mut BTreeMap<ObjectId, RulesInteger>> {
        Some(match kind {
            CounterKind::Vow => &mut self.vow_counters,
            CounterKind::Corpse => &mut self.corpse_counters,
            CounterKind::Blessing => &mut self.blessing_counters,
            CounterKind::Despair => &mut self.despair_counters,
            CounterKind::Hatchling => &mut self.hatchling_counters,
            CounterKind::Indestructible => &mut self.indestructible_counters,
            CounterKind::Page => &mut self.page_counters,
            CounterKind::Quest => &mut self.quest_counters,
            CounterKind::Verse => &mut self.verse_counters,
            CounterKind::Suspect => &mut self.suspect_counters,
            CounterKind::Charge => &mut self.charge,
            CounterKind::Defense => &mut self.defense,
            CounterKind::Loyalty => &mut self.loyalty,
            CounterKind::Storage => &mut self.storage,
            CounterKind::Lore => &mut self.lore,
            CounterKind::Time => &mut self.time_counters,
            CounterKind::Fade => &mut self.fade,
            CounterKind::Age => &mut self.age,
            CounterKind::PlusOne
            | CounterKind::MinusOne
            | CounterKind::Experience
            | CounterKind::Poison
            | CounterKind::Energy => return None,
            _ => self.named_counters.entry(kind).or_default(),
        })
    }
    pub(crate) fn permanent_counter_count(
        &self,
        object: ObjectId,
        kind: CounterKind,
    ) -> RulesInteger {
        let counters = match kind {
            CounterKind::Vow => &self.vow_counters,
            CounterKind::Corpse => &self.corpse_counters,
            CounterKind::Blessing => &self.blessing_counters,
            CounterKind::Despair => &self.despair_counters,
            CounterKind::Hatchling => &self.hatchling_counters,
            CounterKind::Indestructible => &self.indestructible_counters,
            CounterKind::Page => &self.page_counters,
            CounterKind::Quest => &self.quest_counters,
            CounterKind::Verse => &self.verse_counters,
            CounterKind::Suspect => &self.suspect_counters,
            CounterKind::Charge => &self.charge,
            CounterKind::Defense => &self.defense,
            CounterKind::Loyalty => &self.loyalty,
            CounterKind::Storage => &self.storage,
            CounterKind::Lore => &self.lore,
            CounterKind::Time => &self.time_counters,
            CounterKind::Fade => &self.fade,
            CounterKind::Age => &self.age,
            CounterKind::PlusOne => return self.counters(object).plus_one,
            CounterKind::MinusOne => return self.counters(object).minus_one,
            CounterKind::Poison | CounterKind::Experience | CounterKind::Energy => return 0.into(),
            _ => {
                return self
                    .named_counters
                    .get(&kind)
                    .and_then(|row| row.get(&object))
                    .cloned()
                    .unwrap_or_else(|| 0.into());
            }
        };
        counters.get(&object).cloned().unwrap_or_else(|| 0.into())
    }
}

impl EffectWindow {
    /// Number of counters of the given kind on this battlefield incarnation.
    pub fn counters_of_kind(&self, object: ObjectId, kind: CounterKind) -> RulesInteger {
        self.characteristics.permanent_counter_count(object, kind)
    }

    /// Resolve a source-relative counter expression from the current or last battlefield appearance.
    pub(super) fn source_counter_value_at_resolution(
        &self,
        entry: &crate::stack_window::StackEntry,
        kind: CounterKind,
    ) -> Result<RulesInteger, EffectError> {
        let source = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                source.id()
            }
            _ => return Err(EffectError::InconsistentProgram),
        };
        if self
            .window
            .cards()
            .get(source)
            .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        {
            return Ok(self.counters_of_kind(source, kind));
        }
        let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id()) else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(payload
            .departed_source_counters
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| 0.into()))
    }

    /// Current count of a player counter kind on a player.
    pub fn player_counters_of_kind(&self, player: SeatId, kind: CounterKind) -> RulesInteger {
        match kind {
            CounterKind::Poison => self.poison_counters(player),
            CounterKind::Experience => self.experience_counters(player),
            _ => self
                .characteristics
                .player_counters
                .get(&kind)
                .and_then(|row| row.get(&player))
                .cloned()
                .unwrap_or_else(|| 0.into()),
        }
    }

    pub(super) fn give_player_counters(
        &mut self,
        player: SeatId,
        kind: CounterKind,
        amount: &RulesInteger,
        entry: StackEntryId,
        source: crate::damage::DamageSourceId,
        controller: SeatId,
    ) -> Result<Option<RulesOccurrence>, EffectError> {
        if !amount.is_positive() {
            return Ok(None);
        }
        match kind {
            CounterKind::Poison => {
                let before = self.poison_counters(player);
                let after = before.plus(amount);
                self.poison.insert(player, after.clone());
                Ok(Some(RulesOccurrence::PlayerPoisoned {
                    player,
                    before,
                    after,
                    contributions: vec![crate::occurrence::PoisonContribution {
                        origin: crate::occurrence::PoisonOrigin::Effect(entry),
                        source,
                        controller,
                        amount: amount.clone(),
                    }],
                }))
            }
            CounterKind::Experience => {
                let before = self.experience_counters(player);
                let after = before.plus(amount);
                self.characteristics
                    .experience
                    .insert(player, after.clone());
                Ok(Some(RulesOccurrence::ExperienceCountersChanged {
                    player,
                    before,
                    after,
                }))
            }
            CounterKind::Energy => {
                let before = self.player_counters_of_kind(player, kind);
                let after = before.plus(amount);
                self.characteristics
                    .player_counters
                    .entry(kind)
                    .or_default()
                    .insert(player, after.clone());
                Ok(Some(RulesOccurrence::PlayerCounterChanged {
                    player,
                    kind,
                    before,
                    after,
                }))
            }
            _ => Err(EffectError::InconsistentProgram),
        }
    }

    pub(super) fn give_experience_counter(&mut self, player: SeatId) -> RulesOccurrence {
        let before = self.experience_counters(player);
        let after = before.plus(&1.into());
        self.characteristics
            .experience
            .insert(player, after.clone());
        RulesOccurrence::ExperienceCountersChanged {
            player,
            before,
            after,
        }
    }

    pub(super) fn proliferate_choice(&self) -> Result<ProliferateChoice, EffectError> {
        if !matches!(
            self.current_instruction()?,
            Instruction::Proliferate | Instruction::DoubleCountersOnTargets
        ) {
            return Err(EffectError::WrongResolutionFrame);
        }
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self
            .window
            .resolving()
            .filter(|entry| entry.id() == cursor.entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        let mut domain = BTreeMap::new();
        for object in self
            .window
            .cards()
            .iter()
            .filter(|o| o.zone() == ObjectZone::Battlefield)
        {
            let id = object.id();
            let kinds: BTreeSet<_> = CounterKind::PERMANENT
                .into_iter()
                .filter(|kind| {
                    self.characteristics
                        .permanent_counter_count(id, *kind)
                        .is_positive()
                })
                .collect();
            if !kinds.is_empty() {
                domain.insert(TargetRef::Object(id), kinds);
            }
        }
        for &player in self.window.participants() {
            let mut counters = BTreeSet::new();
            if self.poison_counters(player).is_positive() {
                counters.insert(CounterKind::Poison);
            }
            if self.experience_counters(player).is_positive() {
                counters.insert(CounterKind::Experience);
            }
            if self
                .player_counters_of_kind(player, CounterKind::Energy)
                .is_positive()
            {
                counters.insert(CounterKind::Energy);
            }
            if !counters.is_empty() {
                domain.insert(TargetRef::Player(player), counters);
            }
        }
        Ok(ProliferateChoice {
            revision: self.revision,
            entry: entry.id(),
            instruction: cursor.instruction,
            chooser: entry.controller(),
            domain,
        })
    }

    pub(super) fn proliferate(
        &mut self,
        frame: &ProliferateChoice,
        chosen: Vec<TargetRef>,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.increase_existing_counters(frame, chosen, false, journal)
    }

    pub(super) fn double_target_counters(
        &mut self,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let frame = self.proliferate_choice()?;
        let chosen = self
            .legal_permanent_targets()?
            .into_iter()
            .map(TargetRef::Object)
            .filter(|target| frame.domain.contains_key(target))
            .collect();
        self.increase_existing_counters(&frame, chosen, true, journal)
    }

    fn increase_existing_counters(
        &mut self,
        frame: &ProliferateChoice,
        chosen: Vec<TargetRef>,
        double: bool,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let chosen_set = chosen.iter().copied().collect::<BTreeSet<_>>();
        if chosen.len() != chosen_set.len()
            || chosen_set
                .iter()
                .any(|target| !frame.domain.contains_key(target))
        {
            return Err(EffectError::IllegalTarget);
        }
        if double && chosen_set.is_empty() {
            return self.advance_instruction();
        }
        let before = self.rules_snapshot(None)?;
        let mut selected = BTreeMap::new();
        let mut pt_updates = Vec::new();
        let mut changes = Vec::new();
        for selected_object in chosen_set {
            let kinds = frame
                .domain
                .get(&selected_object)
                .ok_or(EffectError::IllegalTarget)?;
            selected.insert(selected_object, kinds.clone());
            match selected_object {
                TargetRef::Player(player) => {
                    if kinds.contains(&CounterKind::Poison) {
                        let count = self
                            .poison
                            .get_mut(&player)
                            .ok_or(EffectError::InconsistentProgram)?;
                        *count = count.plus(&1.into());
                    }
                    if kinds.contains(&CounterKind::Experience) {
                        changes.push(self.give_experience_counter(player));
                    }
                    if kinds.contains(&CounterKind::Energy) {
                        let before = self.player_counters_of_kind(player, CounterKind::Energy);
                        let after = before.plus(&1.into());
                        self.characteristics
                            .player_counters
                            .entry(CounterKind::Energy)
                            .or_default()
                            .insert(player, after.clone());
                        changes.push(RulesOccurrence::PlayerCounterChanged {
                            player,
                            kind: CounterKind::Energy,
                            before,
                            after,
                        });
                    }
                }
                TargetRef::Object(object) => {
                    let multiplier = super::permanent_counters::effect_counter_multiplier(
                        &before.view().evaluated,
                        object,
                    );
                    let current_pt = self.characteristics.counters(object);
                    let mut pt = PowerToughnessCounters::default();
                    for &kind in kinds {
                        match kind {
                            CounterKind::PlusOne => {
                                pt.plus_one = if double {
                                    current_pt.plus_one.clone()
                                } else {
                                    1.into()
                                };
                                continue;
                            }
                            CounterKind::MinusOne => {
                                pt.minus_one = if double {
                                    current_pt.minus_one.clone()
                                } else {
                                    1.into()
                                };
                                continue;
                            }
                            _ => {}
                        }
                        let counts = self
                            .characteristics
                            .named_permanent_counters_mut(kind)
                            .ok_or(EffectError::InconsistentProgram)?;
                        let count = counts
                            .get_mut(&object)
                            .ok_or(EffectError::InconsistentProgram)?;
                        let old = count.clone();
                        let added = if double {
                            count.times(&multiplier)
                        } else {
                            multiplier.clone()
                        };
                        *count = count.plus(&added);
                        match kind {
                            CounterKind::Loyalty => {
                                changes.push(RulesOccurrence::LoyaltyCountersChanged {
                                    object,
                                    before: old,
                                    after: count.clone(),
                                })
                            }
                            CounterKind::Indestructible => {
                                changes.push(RulesOccurrence::PermanentCounterChanged {
                                    object,
                                    kind,
                                    before: old,
                                    after: count.clone(),
                                });
                                self.characteristics
                                    .refresh_indestructible_counter_grant(frame.entry, object)?;
                            }
                            CounterKind::Page
                            | CounterKind::Quest
                            | CounterKind::Verse
                            | CounterKind::Vow
                            | CounterKind::Corpse
                            | CounterKind::Blessing
                            | CounterKind::Despair
                            | CounterKind::Hatchling
                            | CounterKind::Study
                            | CounterKind::Eyeball
                            | CounterKind::Slime => {
                                changes.push(RulesOccurrence::PermanentCounterChanged {
                                    object,
                                    kind,
                                    before: old,
                                    after: count.clone(),
                                })
                            }
                            CounterKind::Suspect => {
                                changes.push(RulesOccurrence::SuspectCountersChanged {
                                    object,
                                    before: old,
                                    after: count.clone(),
                                })
                            }
                            CounterKind::Charge => {
                                changes.push(RulesOccurrence::ChargeCountersChanged {
                                    object,
                                    before: old,
                                    after: count.clone(),
                                })
                            }
                            CounterKind::Storage => {
                                changes.push(RulesOccurrence::StorageCountersChanged {
                                    object,
                                    before: old,
                                    after: count.clone(),
                                })
                            }
                            CounterKind::Age => changes.push(RulesOccurrence::AgeCounterPlaced {
                                object,
                                before: old,
                                after: count.clone(),
                            }),
                            other => {
                                changes.push(RulesOccurrence::PermanentCounterChanged {
                                    object,
                                    kind: other,
                                    before: old,
                                    after: count.clone(),
                                });
                                if other.keyword_counter_ability().is_some() {
                                    self.characteristics.refresh_keyword_counter_grant(
                                        frame.entry,
                                        object,
                                        other,
                                    )?;
                                }
                            }
                        }
                    }
                    if pt != PowerToughnessCounters::default() {
                        let pt =
                            self.adjusted_pt_counters(&before.view().evaluated, object, pt, None)?;
                        self.characteristics.place_counters(object, &pt)?;
                        pt_updates.push((object, pt));
                    }
                }
                TargetRef::Stack(_) => return Err(EffectError::IllegalTarget),
            }
        }
        self.emit_from(
            EventCause::Instruction {
                entry: frame.entry,
                instruction: frame.instruction,
            },
            &before,
            None,
            journal,
            |old, new, _| {
                for (object, added) in pt_updates {
                    changes.push(RulesOccurrence::CountersPlaced {
                        object,
                        added,
                        before: old.characteristics.counters(object),
                        after: new.characteristics.counters(object),
                        before_appearance: Box::new(
                            old.appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            new.appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                if double {
                    changes.push(RulesOccurrence::PermanentCountersDoubled { selected });
                } else {
                    changes.push(RulesOccurrence::Proliferated {
                        player: frame.chooser,
                        selected,
                    });
                }
                Ok(changes)
            },
        )?;
        self.advance_instruction()
    }
}
