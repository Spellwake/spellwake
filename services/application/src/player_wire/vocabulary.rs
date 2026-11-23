//! Exact scalar choice vocabulary; no externally tagged object aliases.
use super::*;
/// Public descriptor category, not a serialized privileged RuleFrame.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum DecisionKind {
    LibrarySearch,
    SelectResolution,
    EntryReveal,
    LinkedReturnOrder,
    LinkedAuraChoice,
    CastRecovery,
    OrderResolutionGraveyard,
    OptionalEffect,
    ZoneReplacement,
    ResolutionPayment,
    TransformOrder,
    TimestampOrder,
    Discard,
    OrderGraveyard,
    ReturnCommanders,
    KeepLegend,
    OrderTriggers,
    PlaneswalkerType,
    AttackDirection,
    OpponentChoice,
    ReplacementChooser,
    DamageRecipient,
    EntryPlayers,
    MyriadDefender,
    ReselectAttackDefender,
    TokenDefender,
    TriggerTarget,
    Proliferate,
    CounterRemoval,
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
            "library-search" => Ok(Self::LibrarySearch),
            "select-resolution" => Ok(Self::SelectResolution),
            "entry-reveal" => Ok(Self::EntryReveal),
            "linked-return-order" => Ok(Self::LinkedReturnOrder),
            "linked-aura-choice" => Ok(Self::LinkedAuraChoice),
            "cast-recovery" => Ok(Self::CastRecovery),
            "order-resolution-graveyard" => Ok(Self::OrderResolutionGraveyard),
            "zone-replacement" => Ok(Self::ZoneReplacement),
            "optional-effect" => Ok(Self::OptionalEffect),
            "resolution-payment" => Ok(Self::ResolutionPayment),
            "transform-order" => Ok(Self::TransformOrder),
            "timestamp-order" => Ok(Self::TimestampOrder),
            "discard" => Ok(Self::Discard),
            "return-commanders" => Ok(Self::ReturnCommanders),
            "order-graveyard" => Ok(Self::OrderGraveyard),
            "keep-legend" => Ok(Self::KeepLegend),
            "order-triggers" => Ok(Self::OrderTriggers),
            "planeswalker-type" => Ok(Self::PlaneswalkerType),
            "attack-direction" => Ok(Self::AttackDirection),
            "opponent-choice" => Ok(Self::OpponentChoice),
            "replacement-chooser" => Ok(Self::ReplacementChooser),
            "damage-recipient" => Ok(Self::DamageRecipient),
            "entry-players" => Ok(Self::EntryPlayers),
            "myriad-defender" => Ok(Self::MyriadDefender),
            "reselect-attack-defender" => Ok(Self::ReselectAttackDefender),
            "token-defender" => Ok(Self::TokenDefender),
            "trigger-target" => Ok(Self::TriggerTarget),
            "proliferate" => Ok(Self::Proliferate),
            "counter-removal" => Ok(Self::CounterRemoval),
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
    Battle,
    Planeswalker,
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
            "battle" => Ok(Self::Battle),
            "planeswalker" => Ok(Self::Planeswalker),
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
    Partner,
    ChooseABackground,
    DoctorsCompanion,
    Bargain,
    Banding,
    BandsWithOtherLegendaryCreatures,
    BandsWithOtherCreaturesNamedWolvesOfTheHunt,
    Vigilance,
    Flying,
    Reach,
    Decayed,
    Daybound,
    Nightbound,
    Phasing,
    Defender,
    Menace,
    Shadow,
    Horsemanship,
    Fear,
    Intimidate,
    Skulk,
    Plainswalk,
    Islandwalk,
    Swampwalk,
    Mountainwalk,
    Forestwalk,
    Desertwalk,
    NonbasicLandwalk,
    LegendaryLandwalk,
    SnowLandwalk,
    SnowForestwalk,
    SnowSwampwalk,
    ArtifactLandwalk,
    Trample,
    Deathtouch,
    Convoke,
    Improvise,
    ProtectionFromWhite,
    ProtectionFromBlue,
    ProtectionFromBlack,
    ProtectionFromRed,
    ProtectionFromGreen,
    ProtectionFromMulticolored,
    ProtectionFromCreatureSubtype,
    DamageCannotBePrevented,
    Retrace,
    JumpStart,
    Rebound,
    Delve,
    Lifelink,
    Indestructible,
    FirstStrike,
    DoubleStrike,
    Shroud,
    Hexproof,
    Flash,
    SplitSecond,
    Changeling,
    Devoid,
    StartYourEngines,
    Exalted,
    Wither,
    Infect,
    Poisonous,
    Toxic,
    Echo,
    CumulativeUpkeep,
    Fading,
    Vanishing,
    Mentor,
    Evolve,
    Backup,
    Extort,
    Increment,
    Mobilize,
    PowerUp,
    Ingest,
    Annihilator,
    Modular,
    Soulshift,
    Graft,
    Persist,
    Undying,
    Afterlife,
    Fabricate,
    Afflict,
    Dethrone,
    Melee,
    Renown,
    Training,
    Prowess,
    BattleCry,
    Bushido,
    Flanking,
    Frenzy,
    Rampage,
}
impl TryFrom<String> for PlayerKeyword {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "haste" => Ok(Self::Haste),
            "bargain" => Ok(Self::Bargain),
            "partner" => Ok(Self::Partner),
            "choose-a-background" => Ok(Self::ChooseABackground),
            "doctors-companion" => Ok(Self::DoctorsCompanion),
            "bands-with-other-legendary-creatures" => Ok(Self::BandsWithOtherLegendaryCreatures),
            "bands-with-other-creatures-named-wolves-of-the-hunt" => {
                Ok(Self::BandsWithOtherCreaturesNamedWolvesOfTheHunt)
            }
            "banding" => Ok(Self::Banding),
            "vigilance" => Ok(Self::Vigilance),
            "flying" => Ok(Self::Flying),
            "reach" => Ok(Self::Reach),
            "decayed" => Ok(Self::Decayed),
            "daybound" => Ok(Self::Daybound),
            "nightbound" => Ok(Self::Nightbound),
            "phasing" => Ok(Self::Phasing),
            "defender" => Ok(Self::Defender),
            "menace" => Ok(Self::Menace),
            "shadow" => Ok(Self::Shadow),
            "horsemanship" => Ok(Self::Horsemanship),
            "fear" => Ok(Self::Fear),
            "intimidate" => Ok(Self::Intimidate),
            "skulk" => Ok(Self::Skulk),
            "plainswalk" => Ok(Self::Plainswalk),
            "islandwalk" => Ok(Self::Islandwalk),
            "swampwalk" => Ok(Self::Swampwalk),
            "mountainwalk" => Ok(Self::Mountainwalk),
            "forestwalk" => Ok(Self::Forestwalk),
            "desertwalk" => Ok(Self::Desertwalk),
            "nonbasic-landwalk" => Ok(Self::NonbasicLandwalk),
            "legendary-landwalk" => Ok(Self::LegendaryLandwalk),
            "snow-landwalk" => Ok(Self::SnowLandwalk),
            "snow-forestwalk" => Ok(Self::SnowForestwalk),
            "snow-swampwalk" => Ok(Self::SnowSwampwalk),
            "artifact-landwalk" => Ok(Self::ArtifactLandwalk),
            "trample" => Ok(Self::Trample),
            "deathtouch" => Ok(Self::Deathtouch),
            "convoke" => Ok(Self::Convoke),
            "delve" => Ok(Self::Delve),
            "improvise" => Ok(Self::Improvise),
            "protection-from-white" => Ok(Self::ProtectionFromWhite),
            "protection-from-blue" => Ok(Self::ProtectionFromBlue),
            "protection-from-black" => Ok(Self::ProtectionFromBlack),
            "protection-from-red" => Ok(Self::ProtectionFromRed),
            "damage-cannot-be-prevented" => Ok(Self::DamageCannotBePrevented),
            "protection-from-creature-subtype" => Ok(Self::ProtectionFromCreatureSubtype),
            "protection-from-multicolored" => Ok(Self::ProtectionFromMulticolored),
            "protection-from-green" => Ok(Self::ProtectionFromGreen),
            "retrace" => Ok(Self::Retrace),
            "jump-start" => Ok(Self::JumpStart),
            "rebound" => Ok(Self::Rebound),
            "lifelink" => Ok(Self::Lifelink),
            "indestructible" => Ok(Self::Indestructible),
            "first-strike" => Ok(Self::FirstStrike),
            "double-strike" => Ok(Self::DoubleStrike),
            "shroud" => Ok(Self::Shroud),
            "hexproof" => Ok(Self::Hexproof),
            "flash" => Ok(Self::Flash),
            "split-second" => Ok(Self::SplitSecond),
            "changeling" => Ok(Self::Changeling),
            "start-your-engines" => Ok(Self::StartYourEngines),
            "devoid" => Ok(Self::Devoid),
            "exalted" => Ok(Self::Exalted),
            "wither" => Ok(Self::Wither),
            "infect" => Ok(Self::Infect),
            "poisonous" => Ok(Self::Poisonous),
            "toxic" => Ok(Self::Toxic),
            "echo" => Ok(Self::Echo),
            "cumulative-upkeep" => Ok(Self::CumulativeUpkeep),
            "fading" => Ok(Self::Fading),
            "mentor" => Ok(Self::Mentor),
            "evolve" => Ok(Self::Evolve),
            "modular" => Ok(Self::Modular),
            "soulshift" => Ok(Self::Soulshift),
            "graft" => Ok(Self::Graft),
            "mobilize" => Ok(Self::Mobilize),
            "power-up" => Ok(Self::PowerUp),
            "ingest" => Ok(Self::Ingest),
            "annihilator" => Ok(Self::Annihilator),
            "increment" => Ok(Self::Increment),
            "extort" => Ok(Self::Extort),
            "backup" => Ok(Self::Backup),
            "vanishing" => Ok(Self::Vanishing),
            "persist" => Ok(Self::Persist),
            "undying" => Ok(Self::Undying),
            "afterlife" => Ok(Self::Afterlife),
            "fabricate" => Ok(Self::Fabricate),
            "afflict" => Ok(Self::Afflict),
            "dethrone" => Ok(Self::Dethrone),
            "melee" => Ok(Self::Melee),
            "renown" => Ok(Self::Renown),
            "training" => Ok(Self::Training),
            "prowess" => Ok(Self::Prowess),
            "battle-cry" => Ok(Self::BattleCry),
            "bushido" => Ok(Self::Bushido),
            "flanking" => Ok(Self::Flanking),
            "frenzy" => Ok(Self::Frenzy),
            "rampage" => Ok(Self::Rampage),
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
    TapChosenPermanent,
    Crew,
    Saddle,
    Station,
    ActivatedUntargeted,
    Ninjutsu,
    Transmute,
    Cycling,
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
            "crew" => Ok(Self::Crew),
            "tap-chosen-permanent" => Ok(Self::TapChosenPermanent),
            "station" => Ok(Self::Station),
            "saddle" => Ok(Self::Saddle),
            "activated-untargeted" => Ok(Self::ActivatedUntargeted),
            "ninjutsu" => Ok(Self::Ninjutsu),
            "transmute" => Ok(Self::Transmute),
            "cycling" => Ok(Self::Cycling),
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
            PlayerAbilityKind::Crew => "crew".to_owned(),
            PlayerAbilityKind::TapChosenPermanent => "tap-chosen-permanent".to_owned(),
            PlayerAbilityKind::Station => "station".to_owned(),
            PlayerAbilityKind::Saddle => "saddle".to_owned(),
            PlayerAbilityKind::ActivatedUntargeted => "activated-untargeted".to_owned(),
            PlayerAbilityKind::Ninjutsu => "ninjutsu".to_owned(),
            PlayerAbilityKind::Transmute => "transmute".to_owned(),
            PlayerAbilityKind::Cycling => "cycling".to_owned(),
            PlayerAbilityKind::Activated => "activated".to_owned(),
            PlayerAbilityKind::Triggered => "triggered".to_owned(),
            PlayerAbilityKind::Continuous => "continuous".to_owned(),
        }
    }
}
