//! Exact scalar choice vocabulary; no externally tagged object aliases.
use super::*;
/// Public descriptor category, not a serialized privileged RuleFrame.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum DecisionKind {
    Scry,
    Surveil,
    OrderLibraryGraveyard,
    ZoneReplacement,
    ReplacementChooser,
    SelectResolution,
    OrderResolutionGraveyard,
    OptionalEffect,
    ResolutionPayment,
    TimestampOrder,
    Discard,
    OrderGraveyard,
    KeepLegend,
    OrderTriggers,
    TriggerTarget,
    DeclareAttackers,
    DeclareBlockers,
    AssignCombatDamage,
    Mulligan,
    Bottom,
    CopyTargets,
    DamageOrder,
    AllocatePrevention,
    ManaColor,
    CreatureType,
    Modes,
    CardName,
    Number,
    Distribution,
}
impl TryFrom<String> for DecisionKind {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "scry" => Ok(Self::Scry),
            "surveil" => Ok(Self::Surveil),
            "order-library-graveyard" => Ok(Self::OrderLibraryGraveyard),
            "zone-replacement" => Ok(Self::ZoneReplacement),
            "replacement-chooser" => Ok(Self::ReplacementChooser),
            "select-resolution" => Ok(Self::SelectResolution),
            "order-resolution-graveyard" => Ok(Self::OrderResolutionGraveyard),
            "optional-effect" => Ok(Self::OptionalEffect),
            "resolution-payment" => Ok(Self::ResolutionPayment),
            "timestamp-order" => Ok(Self::TimestampOrder),
            "discard" => Ok(Self::Discard),
            "order-graveyard" => Ok(Self::OrderGraveyard),
            "keep-legend" => Ok(Self::KeepLegend),
            "order-triggers" => Ok(Self::OrderTriggers),
            "trigger-target" => Ok(Self::TriggerTarget),
            "declare-attackers" => Ok(Self::DeclareAttackers),
            "declare-blockers" => Ok(Self::DeclareBlockers),
            "assign-combat-damage" => Ok(Self::AssignCombatDamage),
            "mulligan" => Ok(Self::Mulligan),
            "bottom" => Ok(Self::Bottom),
            "copy-targets" => Ok(Self::CopyTargets),
            "damage-order" => Ok(Self::DamageOrder),
            "allocate-prevention" => Ok(Self::AllocatePrevention),
            "mana-color" => Ok(Self::ManaColor),
            "creature-type" => Ok(Self::CreatureType),
            "modes" => Ok(Self::Modes),
            "card-name" => Ok(Self::CardName),
            "number" => Ok(Self::Number),
            "distribution" => Ok(Self::Distribution),
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// Disclosure envelope; actual viewer authorization remains a trusted fact.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum DecisionVisibility {
    Public,
    RespondingSeat,
    SealedUntilComplete,
}
impl TryFrom<String> for DecisionVisibility {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "public" => Ok(Self::Public),
            "responding-seat" => Ok(Self::RespondingSeat),
            "sealed-until-complete" => Ok(Self::SealedUntilComplete),
            _ => Err(InvalidPlayerWire),
        }
    }
}

/// Public finite vocabulary; source coverage does not imply a native adapter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum PlayerZoneKind {
    Library,
    Hand,
    Graveyard,
    Battlefield,
    Stack,
    Exile,
    Command,
}
impl TryFrom<String> for PlayerZoneKind {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "library" => Ok(Self::Library),
            "hand" => Ok(Self::Hand),
            "graveyard" => Ok(Self::Graveyard),
            "battlefield" => Ok(Self::Battlefield),
            "stack" => Ok(Self::Stack),
            "exile" => Ok(Self::Exile),
            "command" => Ok(Self::Command),
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// Public finite vocabulary; source coverage does not imply a native adapter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum PlayerPhase {
    Beginning,
    PrecombatMain,
    Combat,
    PostcombatMain,
    Ending,
}
impl TryFrom<String> for PlayerPhase {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "beginning" => Ok(Self::Beginning),
            "precombat-main" => Ok(Self::PrecombatMain),
            "combat" => Ok(Self::Combat),
            "postcombat-main" => Ok(Self::PostcombatMain),
            "ending" => Ok(Self::Ending),
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// Public finite vocabulary; source coverage does not imply a native adapter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum PlayerStep {
    Untap,
    Upkeep,
    Draw,
    PrecombatMain,
    BeginningCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndCombat,
    PostcombatMain,
    End,
    Cleanup,
}
impl TryFrom<String> for PlayerStep {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "untap" => Ok(Self::Untap),
            "upkeep" => Ok(Self::Upkeep),
            "draw" => Ok(Self::Draw),
            "precombat-main" => Ok(Self::PrecombatMain),
            "beginning-combat" => Ok(Self::BeginningCombat),
            "declare-attackers" => Ok(Self::DeclareAttackers),
            "declare-blockers" => Ok(Self::DeclareBlockers),
            "combat-damage" => Ok(Self::CombatDamage),
            "end-combat" => Ok(Self::EndCombat),
            "postcombat-main" => Ok(Self::PostcombatMain),
            "end" => Ok(Self::End),
            "cleanup" => Ok(Self::Cleanup),
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// Public finite vocabulary; source coverage does not imply a native adapter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum PlayerCardType {
    Land,
    Artifact,
    Creature,
    Enchantment,
    Kindred,
    Instant,
    Sorcery,
}
impl TryFrom<String> for PlayerCardType {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "land" => Ok(Self::Land),
            "artifact" => Ok(Self::Artifact),
            "creature" => Ok(Self::Creature),
            "enchantment" => Ok(Self::Enchantment),
            "kindred" => Ok(Self::Kindred),
            "instant" => Ok(Self::Instant),
            "sorcery" => Ok(Self::Sorcery),
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// Public finite vocabulary; source coverage does not imply a native adapter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum PlayerKeyword {
    Haste,
    Vigilance,
}
impl TryFrom<String> for PlayerKeyword {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "haste" => Ok(Self::Haste),
            "vigilance" => Ok(Self::Vigilance),
            _ => Err(InvalidPlayerWire),
        }
    }
}

/// Finite public projection vocabulary, not native source qualification.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum PlayerSupertype {
    Basic,
    Legendary,
    Ongoing,
    Snow,
    World,
}
impl TryFrom<String> for PlayerSupertype {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "basic" => Ok(Self::Basic),
            "legendary" => Ok(Self::Legendary),
            "ongoing" => Ok(Self::Ongoing),
            "snow" => Ok(Self::Snow),
            "world" => Ok(Self::World),
            _ => Err(InvalidPlayerWire),
        }
    }
}
impl From<PlayerSupertype> for String {
    fn from(v: PlayerSupertype) -> Self {
        match v {
            PlayerSupertype::Basic => "basic".to_owned(),
            PlayerSupertype::Legendary => "legendary".to_owned(),
            PlayerSupertype::Ongoing => "ongoing".to_owned(),
            PlayerSupertype::Snow => "snow".to_owned(),
            PlayerSupertype::World => "world".to_owned(),
        }
    }
}
/// Finite public projection vocabulary, not native source qualification.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum PlayerLandType {
    Cave,
    Desert,
    Forest,
    Gate,
    Island,
    Lair,
    Locus,
    Mine,
    Mountain,
    Plains,
    Planet,
    PowerPlant,
    Sphere,
    Swamp,
    Tower,
    Town,
    Urzas,
}
impl TryFrom<String> for PlayerLandType {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "Cave" => Ok(Self::Cave),
            "Desert" => Ok(Self::Desert),
            "Forest" => Ok(Self::Forest),
            "Gate" => Ok(Self::Gate),
            "Island" => Ok(Self::Island),
            "Lair" => Ok(Self::Lair),
            "Locus" => Ok(Self::Locus),
            "Mine" => Ok(Self::Mine),
            "Mountain" => Ok(Self::Mountain),
            "Plains" => Ok(Self::Plains),
            "Planet" => Ok(Self::Planet),
            "PowerPlant" => Ok(Self::PowerPlant),
            "Sphere" => Ok(Self::Sphere),
            "Swamp" => Ok(Self::Swamp),
            "Tower" => Ok(Self::Tower),
            "Town" => Ok(Self::Town),
            "Urzas" => Ok(Self::Urzas),
            _ => Err(InvalidPlayerWire),
        }
    }
}
impl From<PlayerLandType> for String {
    fn from(v: PlayerLandType) -> Self {
        match v {
            PlayerLandType::Cave => "Cave".to_owned(),
            PlayerLandType::Desert => "Desert".to_owned(),
            PlayerLandType::Forest => "Forest".to_owned(),
            PlayerLandType::Gate => "Gate".to_owned(),
            PlayerLandType::Island => "Island".to_owned(),
            PlayerLandType::Lair => "Lair".to_owned(),
            PlayerLandType::Locus => "Locus".to_owned(),
            PlayerLandType::Mine => "Mine".to_owned(),
            PlayerLandType::Mountain => "Mountain".to_owned(),
            PlayerLandType::Plains => "Plains".to_owned(),
            PlayerLandType::Planet => "Planet".to_owned(),
            PlayerLandType::PowerPlant => "PowerPlant".to_owned(),
            PlayerLandType::Sphere => "Sphere".to_owned(),
            PlayerLandType::Swamp => "Swamp".to_owned(),
            PlayerLandType::Tower => "Tower".to_owned(),
            PlayerLandType::Town => "Town".to_owned(),
            PlayerLandType::Urzas => "Urzas".to_owned(),
        }
    }
}
/// Finite public projection vocabulary, not native source qualification.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum PlayerAbilityKind {
    Mana,
    Activated,
    Triggered,
    Continuous,
}
impl TryFrom<String> for PlayerAbilityKind {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "mana" => Ok(Self::Mana),
            "activated" => Ok(Self::Activated),
            "triggered" => Ok(Self::Triggered),
            "continuous" => Ok(Self::Continuous),
            _ => Err(InvalidPlayerWire),
        }
    }
}
impl From<PlayerAbilityKind> for String {
    fn from(v: PlayerAbilityKind) -> Self {
        match v {
            PlayerAbilityKind::Mana => "mana".to_owned(),
            PlayerAbilityKind::Activated => "activated".to_owned(),
            PlayerAbilityKind::Triggered => "triggered".to_owned(),
            PlayerAbilityKind::Continuous => "continuous".to_owned(),
        }
    }
}
