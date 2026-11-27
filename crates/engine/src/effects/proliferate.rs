//! Choose any subset of counter-bearing battlefield permanents and players (CR 701).
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

/// Fixed-capacity deterministic inline key for arbitrary MTG named counters.
/// Stores ASCII-lowercased counter names up to 24 bytes so `CounterKind` remains `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustomCounterKey([u8; 24]);

impl CustomCounterKey {
    /// Construct a normalized custom counter key from a non-empty ASCII label (1..=24 bytes).
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        let trimmed = label.trim();
        let bytes = trimmed.as_bytes();
        if bytes.is_empty() || bytes.len() > 24 {
            return None;
        }
        let mut buf = [0u8; 24];
        for (i, &b) in bytes.iter().enumerate() {
            if !b.is_ascii() || b.is_ascii_control() {
                return None;
            }
            buf[i] = b.to_ascii_lowercase();
        }
        Some(Self(buf))
    }

    /// Borrow the normalized lowercase ASCII counter name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        let len = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        std::str::from_utf8(&self.0[..len]).unwrap_or("")
    }
}

impl std::fmt::Debug for CustomCounterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CustomCounterKey({:?})", self.as_str())
    }
}

/// Counter kinds that can occur on a permanent or player in this engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// Arbitrary non-standard power/toughness counter such as +0/+1, +2/+2, -0/-1, or -2/-2.
    PtDelta {
        power: i8,
        toughness: i8,
    },
    /// Any other named permanent counter (CR 122.1) stored by normalized lowercase label.
    Custom(CustomCounterKey),
    Poison,
    Experience,
    Energy,
    /// CR 725 rad counter on a player.
    Rad,
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
        matches!(
            self,
            Self::Poison | Self::Experience | Self::Energy | Self::Rad
        )
    }

    /// Whether this counter kind can exist on a permanent (CR 122.1).
    #[must_use]
    pub const fn is_permanent(self) -> bool {
        !self.is_player_counter()
    }

    pub(crate) fn is_named(self) -> bool {
        !self.is_player_counter() && !matches!(self, Self::PlusOne | Self::MinusOne)
    }

    /// Canonical lowercase Oracle label for display and wire projection.
    #[must_use]
    pub fn oracle_label(self) -> std::borrow::Cow<'static, str> {
        use std::borrow::Cow;
        match self {
            Self::PlusOne => Cow::Borrowed("+1/+1"),
            Self::MinusOne => Cow::Borrowed("-1/-1"),
            Self::PtDelta { power, toughness } => {
                let ps = if power >= 0 { "+" } else { "" };
                let ts = if toughness >= 0 { "+" } else { "" };
                Cow::Owned(format!("{ps}{power}/{ts}{toughness}"))
            }
            Self::Custom(key) => Cow::Owned(key.as_str().to_owned()),
            Self::Page => Cow::Borrowed("page"),
            Self::Quest => Cow::Borrowed("quest"),
            Self::Verse => Cow::Borrowed("verse"),
            Self::Suspect => Cow::Borrowed("suspect"),
            Self::Charge => Cow::Borrowed("charge"),
            Self::Storage => Cow::Borrowed("storage"),
            Self::Lore => Cow::Borrowed("lore"),
            Self::Loyalty => Cow::Borrowed("loyalty"),
            Self::Defense => Cow::Borrowed("defense"),
            Self::Time => Cow::Borrowed("time"),
            Self::Fade => Cow::Borrowed("fade"),
            Self::Age => Cow::Borrowed("age"),
            Self::Indestructible => Cow::Borrowed("indestructible"),
            Self::Corpse => Cow::Borrowed("corpse"),
            Self::Vow => Cow::Borrowed("vow"),
            Self::Blessing => Cow::Borrowed("blessing"),
            Self::Despair => Cow::Borrowed("despair"),
            Self::Hatchling => Cow::Borrowed("hatchling"),
            Self::Study => Cow::Borrowed("study"),
            Self::Eyeball => Cow::Borrowed("eyeball"),
            Self::Slime => Cow::Borrowed("slime"),
            Self::Oil => Cow::Borrowed("oil"),
            Self::Level => Cow::Borrowed("level"),
            Self::Spore => Cow::Borrowed("spore"),
            Self::Ki => Cow::Borrowed("ki"),
            Self::Soul => Cow::Borrowed("soul"),
            Self::Fuse => Cow::Borrowed("fuse"),
            Self::Depletion => Cow::Borrowed("depletion"),
            Self::Bounty => Cow::Borrowed("bounty"),
            Self::Flood => Cow::Borrowed("flood"),
            Self::Flame => Cow::Borrowed("flame"),
            Self::Hour => Cow::Borrowed("hour"),
            Self::Brick => Cow::Borrowed("brick"),
            Self::Plague => Cow::Borrowed("plague"),
            Self::Doom => Cow::Borrowed("doom"),
            Self::Pressure => Cow::Borrowed("pressure"),
            Self::Feather => Cow::Borrowed("feather"),
            Self::Fate => Cow::Borrowed("fate"),
            Self::Infection => Cow::Borrowed("infection"),
            Self::Ice => Cow::Borrowed("ice"),
            Self::Tide => Cow::Borrowed("tide"),
            Self::Growth => Cow::Borrowed("growth"),
            Self::Divinity => Cow::Borrowed("divinity"),
            Self::Fungus => Cow::Borrowed("fungus"),
            Self::Hit => Cow::Borrowed("hit"),
            Self::Gold => Cow::Borrowed("gold"),
            Self::Blood => Cow::Borrowed("blood"),
            Self::Cage => Cow::Borrowed("cage"),
            Self::Coin => Cow::Borrowed("coin"),
            Self::Credit => Cow::Borrowed("credit"),
            Self::Death => Cow::Borrowed("death"),
            Self::Delay => Cow::Borrowed("delay"),
            Self::Devotion => Cow::Borrowed("devotion"),
            Self::Egg => Cow::Borrowed("egg"),
            Self::Eon => Cow::Borrowed("eon"),
            Self::Fury => Cow::Borrowed("fury"),
            Self::Incubation => Cow::Borrowed("incubation"),
            Self::Knowledge => Cow::Borrowed("knowledge"),
            Self::Luck => Cow::Borrowed("luck"),
            Self::Mine => Cow::Borrowed("mine"),
            Self::Music => Cow::Borrowed("music"),
            Self::Muster => Cow::Borrowed("muster"),
            Self::Net => Cow::Borrowed("net"),
            Self::Omen => Cow::Borrowed("omen"),
            Self::Ore => Cow::Borrowed("ore"),
            Self::Pain => Cow::Borrowed("pain"),
            Self::Petal => Cow::Borrowed("petal"),
            Self::Phylactery => Cow::Borrowed("phylactery"),
            Self::Pin => Cow::Borrowed("pin"),
            Self::Plot => Cow::Borrowed("plot"),
            Self::Prey => Cow::Borrowed("prey"),
            Self::Pupa => Cow::Borrowed("pupa"),
            Self::Shell => Cow::Borrowed("shell"),
            Self::Silver => Cow::Borrowed("silver"),
            Self::Sleep => Cow::Borrowed("sleep"),
            Self::Soot => Cow::Borrowed("soot"),
            Self::Stash => Cow::Borrowed("stash"),
            Self::Strife => Cow::Borrowed("strife"),
            Self::Task => Cow::Borrowed("task"),
            Self::Tower => Cow::Borrowed("tower"),
            Self::Training => Cow::Borrowed("training"),
            Self::Trap => Cow::Borrowed("trap"),
            Self::Treasure => Cow::Borrowed("treasure"),
            Self::Valor => Cow::Borrowed("valor"),
            Self::Velocity => Cow::Borrowed("velocity"),
            Self::Vitality => Cow::Borrowed("vitality"),
            Self::Void => Cow::Borrowed("void"),
            Self::Vortex => Cow::Borrowed("vortex"),
            Self::Wage => Cow::Borrowed("wage"),
            Self::Winch => Cow::Borrowed("winch"),
            Self::Wind => Cow::Borrowed("wind"),
            Self::Wish => Cow::Borrowed("wish"),
            Self::Flying => Cow::Borrowed("flying"),
            Self::FirstStrike => Cow::Borrowed("first strike"),
            Self::DoubleStrike => Cow::Borrowed("double strike"),
            Self::Deathtouch => Cow::Borrowed("deathtouch"),
            Self::Haste => Cow::Borrowed("haste"),
            Self::Hexproof => Cow::Borrowed("hexproof"),
            Self::Lifelink => Cow::Borrowed("lifelink"),
            Self::Menace => Cow::Borrowed("menace"),
            Self::Reach => Cow::Borrowed("reach"),
            Self::Shadow => Cow::Borrowed("shadow"),
            Self::Trample => Cow::Borrowed("trample"),
            Self::Vigilance => Cow::Borrowed("vigilance"),
            Self::Stun => Cow::Borrowed("stun"),
            Self::Shield => Cow::Borrowed("shield"),
            Self::Finality => Cow::Borrowed("finality"),
            Self::Poison => Cow::Borrowed("poison"),
            Self::Experience => Cow::Borrowed("experience"),
            Self::Energy => Cow::Borrowed("energy"),
            Self::Rad => Cow::Borrowed("rad"),
        }
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
            "rad" => Self::Rad,
            other => {
                let trimmed = other.trim();
                if let Some((p_str, t_str)) = trimmed.split_once('/')
                    && (p_str.starts_with('+') || p_str.starts_with('-'))
                    && (t_str.starts_with('+') || t_str.starts_with('-'))
                    && let (Ok(power), Ok(toughness)) = (p_str.parse::<i8>(), t_str.parse::<i8>())
                {
                    return Some(Self::PtDelta { power, toughness });
                }
                if matches!(
                    trimmed,
                    "" | "a"
                        | "an"
                        | "any"
                        | "all"
                        | "that"
                        | "those"
                        | "this"
                        | "the"
                        | "each"
                        | "one"
                        | "two"
                        | "three"
                        | "x"
                        | "counter"
                        | "counters"
                        | "another"
                        | "other"
                        | "more"
                        | "kind"
                        | "type"
                        | "same"
                ) || !trimmed
                    .bytes()
                    .all(|b| b.is_ascii_alphabetic() || matches!(b, b' ' | b'-' | b'\''))
                    || trimmed.contains("  ")
                {
                    return None;
                }
                return CustomCounterKey::from_label(trimmed).map(Self::Custom);
            }
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
    /// Exact map of every permanent counter kind with a positive count currently on `object`.
    pub(crate) fn all_permanent_counters(
        &self,
        object: ObjectId,
    ) -> std::collections::BTreeMap<CounterKind, RulesInteger> {
        let mut out = std::collections::BTreeMap::new();
        let pt = self.counters(object);
        if pt.plus_one.is_positive() {
            out.insert(CounterKind::PlusOne, pt.plus_one.clone());
        }
        if pt.minus_one.is_positive() {
            out.insert(CounterKind::MinusOne, pt.minus_one.clone());
        }
        for (kind, map) in [
            (CounterKind::Vow, &self.vow_counters),
            (CounterKind::Corpse, &self.corpse_counters),
            (CounterKind::Blessing, &self.blessing_counters),
            (CounterKind::Despair, &self.despair_counters),
            (CounterKind::Hatchling, &self.hatchling_counters),
            (CounterKind::Indestructible, &self.indestructible_counters),
            (CounterKind::Page, &self.page_counters),
            (CounterKind::Quest, &self.quest_counters),
            (CounterKind::Verse, &self.verse_counters),
            (CounterKind::Suspect, &self.suspect_counters),
            (CounterKind::Charge, &self.charge),
            (CounterKind::Defense, &self.defense),
            (CounterKind::Loyalty, &self.loyalty),
            (CounterKind::Storage, &self.storage),
            (CounterKind::Lore, &self.lore),
            (CounterKind::Time, &self.time_counters),
            (CounterKind::Fade, &self.fade),
            (CounterKind::Age, &self.age),
        ] {
            if let Some(count) = map.get(&object)
                && count.is_positive()
            {
                out.insert(kind, count.clone());
            }
        }
        for (&kind, map) in &self.named_counters {
            if let Some(count) = map.get(&object)
                && count.is_positive()
            {
                out.insert(kind, count.clone());
            }
        }
        out
    }

    /// Every permanent counter kind with a positive count currently on `object`.
    pub(crate) fn present_permanent_counter_kinds(
        &self,
        object: ObjectId,
    ) -> BTreeSet<CounterKind> {
        let mut kinds = BTreeSet::new();
        let pt = self.counters(object);
        if pt.plus_one.is_positive() {
            kinds.insert(CounterKind::PlusOne);
        }
        if pt.minus_one.is_positive() {
            kinds.insert(CounterKind::MinusOne);
        }
        for (kind, map) in [
            (CounterKind::Vow, &self.vow_counters),
            (CounterKind::Corpse, &self.corpse_counters),
            (CounterKind::Blessing, &self.blessing_counters),
            (CounterKind::Despair, &self.despair_counters),
            (CounterKind::Hatchling, &self.hatchling_counters),
            (CounterKind::Indestructible, &self.indestructible_counters),
            (CounterKind::Page, &self.page_counters),
            (CounterKind::Quest, &self.quest_counters),
            (CounterKind::Verse, &self.verse_counters),
            (CounterKind::Suspect, &self.suspect_counters),
            (CounterKind::Charge, &self.charge),
            (CounterKind::Defense, &self.defense),
            (CounterKind::Loyalty, &self.loyalty),
            (CounterKind::Storage, &self.storage),
            (CounterKind::Lore, &self.lore),
            (CounterKind::Time, &self.time_counters),
            (CounterKind::Fade, &self.fade),
            (CounterKind::Age, &self.age),
        ] {
            if map.get(&object).is_some_and(RulesInteger::is_positive) {
                kinds.insert(kind);
            }
        }
        for (&kind, map) in &self.named_counters {
            if map.get(&object).is_some_and(RulesInteger::is_positive) {
                kinds.insert(kind);
            }
        }
        kinds
    }

    pub(crate) fn total_permanent_counters(&self, object: ObjectId) -> RulesInteger {
        self.present_permanent_counter_kinds(object)
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
            | CounterKind::Energy
            | CounterKind::Rad => return None,
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
            CounterKind::Poison
            | CounterKind::Experience
            | CounterKind::Energy
            | CounterKind::Rad => return 0.into(),
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

    /// All non-dedicated named counters (including keyword, P/T delta, and custom counters)
    /// currently present with positive count on this battlefield incarnation.
    pub fn extra_named_counters(&self, object: ObjectId) -> BTreeMap<CounterKind, RulesInteger> {
        let mut out = BTreeMap::new();
        for (&kind, map) in &self.characteristics.named_counters {
            if let Some(count) = map.get(&object)
                && count.is_positive()
            {
                out.insert(kind, count.clone());
            }
        }
        out
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
        let evaluated = self.evaluated_characteristics()?;
        if self.player_counter_prevented(&evaluated, player, kind)? {
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
            CounterKind::Energy | CounterKind::Rad | CounterKind::Custom(_) => {
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
        let is_proliferate = matches!(
            self.current_instruction()?,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::KeywordAction(
                        crate::toolbox::PlayerKeywordAction::Proliferate
                    )
                )
        );
        if !is_proliferate {
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
            let kinds = self.characteristics.present_permanent_counter_kinds(id);
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
            for kind in [CounterKind::Energy, CounterKind::Rad] {
                if self.player_counters_of_kind(player, kind).is_positive() {
                    counters.insert(kind);
                }
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
        self.increase_existing_counters(frame, chosen, false, None, journal)
    }

    fn increase_existing_counters(
        &mut self,
        frame: &ProliferateChoice,
        chosen: Vec<TargetRef>,
        double: bool,
        kind_filter: Option<CounterKind>,
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
            let all_kinds = frame
                .domain
                .get(&selected_object)
                .ok_or(EffectError::IllegalTarget)?;
            let kinds: BTreeSet<CounterKind> = match kind_filter {
                Some(k) => all_kinds.iter().copied().filter(|x| *x == k).collect(),
                None => all_kinds.clone(),
            };
            if kinds.is_empty() {
                continue;
            }
            selected.insert(selected_object, kinds.clone());
            let kinds = &kinds;
            match selected_object {
                TargetRef::Player(player) => {
                    if kinds.contains(&CounterKind::Poison) {
                        let count = self
                            .poison
                            .get_mut(&player)
                            .ok_or(EffectError::InconsistentProgram)?;
                        let delta = if double { count.clone() } else { 1.into() };
                        *count = count.plus(&delta);
                    }
                    if kinds.contains(&CounterKind::Experience) {
                        if double {
                            let before = self.experience_counters(player);
                            let after = before.plus(&before);
                            self.characteristics
                                .experience
                                .insert(player, after.clone());
                            changes.push(RulesOccurrence::ExperienceCountersChanged {
                                player,
                                before,
                                after,
                            });
                        } else {
                            changes.push(self.give_experience_counter(player));
                        }
                    }
                    for kind in [CounterKind::Energy, CounterKind::Rad] {
                        if kinds.contains(&kind) {
                            let before = self.player_counters_of_kind(player, kind);
                            let delta = if double { before.clone() } else { 1.into() };
                            let after = before.plus(&delta);
                            self.characteristics
                                .player_counters
                                .entry(kind)
                                .or_default()
                                .insert(player, after.clone());
                            changes.push(RulesOccurrence::PlayerCounterChanged {
                                player,
                                kind,
                                before,
                                after,
                            });
                        }
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
                        if pt.valid_placement() {
                            self.characteristics.place_counters(object, &pt)?;
                            pt_updates.push((object, pt));
                        }
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
